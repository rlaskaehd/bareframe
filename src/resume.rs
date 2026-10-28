//! Picking up the agents that ran in bareframe's terminals after it quits. As it quits, it
//! finds each terminal's Claude Code or Codex among the processes under its shell; after the
//! restart, zsh offers `claude --resume <that session>` or `codex resume --last` at the
//! first prompt (see resources/shell-integration/bareframe/.zshenv), to run with Enter.
use bareframe_core::session::SessionKey;
use serde_json::Value;
use std::{collections::HashMap, path::Path, process::Command};

/// Names a terminal's shell among the processes, as `/usr/bin/env` puts it there.
pub const MARKER_VAR: &str = "BAREFRAME_SESSION";
/// The command for zsh to offer at the first prompt.
pub const PREFILL_VAR: &str = "BAREFRAME_RESUME";

/// The marker of the terminal started as `key`: unique to this run of bareframe.
pub fn marker(key: SessionKey) -> String {
    format!("{}-{}-{}", std::process::id(), key.id.0, key.generation)
}

/// What picks up the agent running in each terminal, by its marker; terminals running none
/// are left out.
pub fn commands(markers: &[String]) -> HashMap<String, String> {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,ppid=,args="])
        .output()
    else {
        return HashMap::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let processes: Vec<(u32, u32, &str)> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.trim_start().splitn(2, char::is_whitespace);
            let pid = fields.next()?.parse().ok()?;
            let mut rest = fields.next()?.trim_start().splitn(2, char::is_whitespace);
            let parent = rest.next()?.parse().ok()?;
            Some((pid, parent, rest.next().unwrap_or("").trim_start()))
        })
        .collect();
    markers
        .iter()
        .filter_map(|marker| {
            let named = format!("{MARKER_VAR}={marker} ");
            let (root, _, _) = processes
                .iter()
                .find(|(_, _, args)| args.contains(&named))?;
            Some((marker.clone(), agent_under(*root, &processes)?))
        })
        .collect()
}

/// The agent nearest `root` among the processes under it, as the command resuming it.
fn agent_under(root: u32, processes: &[(u32, u32, &str)]) -> Option<String> {
    let mut level = vec![root];
    while !level.is_empty() {
        let below: Vec<&(u32, u32, &str)> = processes
            .iter()
            .filter(|(_, parent, _)| level.contains(parent))
            .collect();
        if let Some(command) = below.iter().find_map(|(pid, _, args)| resuming(*pid, args)) {
            return Some(command);
        }
        level = below.iter().map(|(pid, _, _)| *pid).collect();
    }
    None
}

/// How to pick up process `pid` again, if it is Claude Code or Codex (as itself, or a
/// script under node or bun).
fn resuming(pid: u32, args: &str) -> Option<String> {
    let mut words = args
        .split_whitespace()
        .map(|word| word.rsplit('/').next().unwrap_or(word));
    let program = match words.next()? {
        "node" | "bun" => words.next()?,
        program => program,
    };
    match program {
        "claude" => Some(claude_session(pid).map_or_else(
            || "claude --continue".to_owned(),
            |id| format!("claude --resume {id}"),
        )),
        // Codex keeps no record of which session a process holds: the folder's latest.
        "codex" => Some("codex resume --last".to_owned()),
        _ => None,
    }
}

/// The session Claude Code process `pid` is in, from the record it keeps while running.
fn claude_session(pid: u32) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let record = Path::new(&home)
        .join(".claude/sessions")
        .join(format!("{pid}.json"));
    let record: Value = serde_json::from_str(&std::fs::read_to_string(record).ok()?).ok()?;
    let id = record["sessionId"].as_str()?;
    // It goes on a command line.
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{agent_under, resuming};

    #[test]
    fn the_agent_nearest_the_shell_is_picked_up() {
        let processes = [
            (
                10,
                1,
                "/usr/bin/login -flp me /bin/bash -c exec env BAREFRAME_SESSION=1-2-0 zsh",
            ),
            (11, 10, "-zsh"),
            (12, 11, "/opt/homebrew/bin/codex"),
            (13, 12, "node /usr/local/bin/claude"),
            (20, 1, "/usr/bin/login -flp me zsh"),
            (21, 20, "-zsh"),
        ];
        assert_eq!(
            agent_under(10, &processes).as_deref(),
            Some("codex resume --last")
        );
        assert_eq!(agent_under(20, &processes), None);
        assert_eq!(
            resuming(0, "node /x/claude --model opus").as_deref(),
            Some("claude --continue")
        );
        assert_eq!(resuming(0, "vim claude"), None);
    }
}
