/// Extract the area name from a file path (first path component, or "(root)").
pub(crate) fn area_of(file: &str) -> &str {
    if let Some(pos) = file.find('/') {
        &file[..pos]
    } else {
        "(root)"
    }
}
