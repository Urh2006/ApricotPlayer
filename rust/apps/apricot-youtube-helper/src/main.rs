use std::io::{self, BufRead, Write};

use apricot_media::{
    MAX_YOUTUBE_MESSAGE_BYTES, YoutubeErrorCode, YoutubeHelperError, YoutubeRequest,
    YoutubeResponse,
};

use apricot_youtube_helper::YoutubeHelper;

#[tokio::main]
async fn main() {
    if let Err(error) = serve().await {
        eprintln!("ApricotPlayer YouTube helper stopped: {error}");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), String> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut helper = YoutubeHelper::new().map_err(|error| error.message)?;

    loop {
        let Some(line) = read_bounded_line(&mut input)? else {
            return Ok(());
        };
        let request = match serde_json::from_slice::<YoutubeRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                let response = YoutubeResponse::failure(
                    0,
                    YoutubeHelperError::new(
                        YoutubeErrorCode::InvalidRequest,
                        format!("Request is not valid JSON: {error}"),
                        false,
                    ),
                );
                write_response(&mut output, &response)?;
                continue;
            }
        };

        let should_shutdown = matches!(request.command, apricot_media::YoutubeCommand::Shutdown);
        let response = helper.handle(request).await;
        write_response(&mut output, &response)?;
        if should_shutdown {
            return Ok(());
        }
    }
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    let read = std::io::Read::take(&mut *reader, (MAX_YOUTUBE_MESSAGE_BYTES + 2) as u64)
        .read_until(b'\n', &mut line)
        .map_err(|error| format!("Could not read request: {error}"))?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() > MAX_YOUTUBE_MESSAGE_BYTES || !line.ends_with(b"\n") {
        return Err(format!(
            "Request exceeded the {MAX_YOUTUBE_MESSAGE_BYTES}-byte protocol limit"
        ));
    }
    line.pop();
    if line.ends_with(b"\r") {
        line.pop();
    }
    Ok(Some(line))
}

fn write_response(writer: &mut impl Write, response: &YoutubeResponse) -> Result<(), String> {
    let encoded = serde_json::to_vec(response)
        .map_err(|error| format!("Could not serialize response: {error}"))?;
    if encoded.len() > MAX_YOUTUBE_MESSAGE_BYTES {
        return Err(format!(
            "Response exceeded the {MAX_YOUTUBE_MESSAGE_BYTES}-byte protocol limit"
        ));
    }
    writer
        .write_all(&encoded)
        .and_then(|()| writer.write_all(b"\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| format!("Could not write response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::read_bounded_line;
    use apricot_media::MAX_YOUTUBE_MESSAGE_BYTES;
    use std::io::Cursor;

    #[test]
    fn bounded_reader_accepts_one_json_line() {
        let mut input = Cursor::new(b"{}\r\n".to_vec());
        assert_eq!(
            read_bounded_line(&mut input).expect("read"),
            Some(b"{}".to_vec())
        );
        assert_eq!(read_bounded_line(&mut input).expect("eof"), None);
    }

    #[test]
    fn bounded_reader_rejects_oversized_input() {
        let mut input = Cursor::new(vec![b'a'; MAX_YOUTUBE_MESSAGE_BYTES + 2]);
        assert!(read_bounded_line(&mut input).is_err());
    }
}
