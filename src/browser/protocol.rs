//! The wire between the app and `bareframe-browser`, shared by both: one JSON object per line
//! over a Unix socket in the user's own temporary folder. A request is
//! `{"id": 1, "method": "tabs", "params": {}}`; its reply is `{"id": 1, "result": …}` or
//! `{"id": 1, "error": {"code": "TAB_CLOSED", "message": "…"}}`.
use std::io::{self, BufRead, Read};

/// The socket's path, given to every terminal bareframe opens while control is on.
pub const ENDPOINT_VAR: &str = "BAREFRAME_BROWSER_ENDPOINT";
/// The terminal's own id: what the user allows is a terminal using a tab.
pub const TERMINAL_VAR: &str = "BAREFRAME_BROWSER_TERMINAL";
/// The `bareframe-browser` executable, for registering it with an agent.
pub const CLI_VAR: &str = "BAREFRAME_BROWSER_CLI";

/// The longest line either side reads: a Retina screenshot in base64 fits well within it.
pub const MAX_LINE: u64 = 64 << 20;

/// The next line from `reader`, without its newline; `None` at the end. A longer line than
/// `MAX_LINE` is an error, not an allocation as large as the sender likes.
pub fn read_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut line = String::new();
    let read = Read::take(&mut *reader, MAX_LINE + 1).read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if read as u64 > MAX_LINE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too long",
        ));
    }
    Ok(Some(line.trim_end_matches(['\n', '\r']).to_owned()))
}
