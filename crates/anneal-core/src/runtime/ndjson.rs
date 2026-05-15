use std::io::{self, Write};

use serde::Serialize;

pub fn write_ndjson<W, I, T>(mut writer: W, rows: I) -> Result<(), NdjsonError>
where
    W: Write,
    I: IntoIterator<Item = T>,
    T: Serialize,
{
    for row in rows {
        serde_json::to_writer(&mut writer, &row)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum NdjsonError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::eval::{Row, Value};
    use std::collections::BTreeMap;

    #[test]
    fn writes_one_json_record_per_line() {
        let row = Row {
            fields: BTreeMap::from([("h".to_string(), Value::String("OQ-22".to_string()))]),
        };
        let mut out = Vec::new();
        write_ndjson(&mut out, [row]).expect("write");
        assert_eq!(String::from_utf8(out).expect("utf8"), "{\"h\":\"OQ-22\"}\n");
    }
}
