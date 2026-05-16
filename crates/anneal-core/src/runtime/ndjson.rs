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

pub fn write_ndjson_with_meta<W, I, T, M>(
    mut writer: W,
    meta: Option<&M>,
    rows: I,
) -> Result<(), NdjsonError>
where
    W: Write,
    I: IntoIterator<Item = T>,
    T: Serialize,
    M: Serialize,
{
    if let Some(meta) = meta {
        serde_json::to_writer(&mut writer, &MetaRecord { meta })?;
        writer.write_all(b"\n")?;
    }
    write_ndjson(writer, rows)
}

#[derive(Serialize)]
struct MetaRecord<'a, T> {
    #[serde(rename = "_meta")]
    meta: &'a T,
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
            derivation: None,
        };
        let mut out = Vec::new();
        write_ndjson(&mut out, [row]).expect("write");
        assert_eq!(String::from_utf8(out).expect("utf8"), "{\"h\":\"OQ-22\"}\n");
    }

    #[test]
    fn writes_optional_meta_record_before_rows() {
        #[derive(Serialize)]
        struct RuntimeMeta<'a> {
            query: &'a str,
            prelude_hash: &'a str,
        }

        let row = Row {
            fields: BTreeMap::from([("h".to_string(), Value::String("OQ-22".to_string()))]),
            derivation: None,
        };
        let mut out = Vec::new();
        write_ndjson_with_meta(
            &mut out,
            Some(&RuntimeMeta {
                query: "? blocked(h).",
                prelude_hash: "abc123",
            }),
            [row],
        )
        .expect("write");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "{\"_meta\":{\"query\":\"? blocked(h).\",\"prelude_hash\":\"abc123\"}}\n{\"h\":\"OQ-22\"}\n"
        );
    }

    #[test]
    fn derivation_field_serializes_as_reserved_output_key() {
        let row = Row {
            fields: BTreeMap::new(),
            derivation: Some(crate::runtime::DerivationNode::synthetic_query(Vec::new())),
        };

        let mut out = Vec::new();
        write_ndjson(&mut out, [row]).expect("write");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "{\"_derivation\":{\"kind\":\"query\",\"label\":\"query output row\"}}\n"
        );
    }
}
