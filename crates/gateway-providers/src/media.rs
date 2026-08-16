pub(crate) fn infer_media_type_from_path(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "html" | "htm" => "text/html",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mpeg" => "video/mpeg",
        "mpegs" | "mpegps" => "video/mpegs",
        "mpg" => "video/mpg",
        "avi" => "video/avi",
        "webm" => "video/webm",
        "wmv" => "video/wmv",
        "flv" => "video/x-flv",
        "3gp" | "3gpp" => "video/3gpp",
        _ => return None,
    })
}

pub(crate) fn is_valid_media_type(value: &str) -> bool {
    let Some((type_name, subtype)) = value.split_once('/') else {
        return false;
    };
    !type_name.is_empty()
        && !subtype.is_empty()
        && !subtype.contains('/')
        && type_name.bytes().all(is_media_type_token_byte)
        && subtype.bytes().all(is_media_type_token_byte)
}

const fn is_media_type_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
mod tests {
    use super::{infer_media_type_from_path, is_valid_media_type};

    #[test]
    fn infers_provider_agnostic_media_types() {
        for (path, expected) in [
            ("clip.webm", "video/webm"),
            ("clip.mov", "video/quicktime"),
            ("clip.flv", "video/x-flv"),
            ("report.PDF", "application/pdf"),
        ] {
            assert_eq!(infer_media_type_from_path(path), Some(expected));
        }
    }

    #[test]
    fn validates_media_type_tokens() {
        for value in ["video/mp4", "application/vnd.example+json", "IMAGE/PNG"] {
            assert!(
                is_valid_media_type(value),
                "expected valid media type: {value}"
            );
        }
        for value in ["video/", "/mp4", "video/not a type", "video/mp4/extra"] {
            assert!(
                !is_valid_media_type(value),
                "expected invalid media type: {value}"
            );
        }
    }
}
