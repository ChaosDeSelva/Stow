//! Talk to Google Chrome through `AppleScript`. Never `tell application "Google Chrome"`
//! before confirming it is running, because that would launch it.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ChromeError {
    NotRunning,
    PermissionDenied,
    NoTab,
    Script(String),
}

impl std::fmt::Display for ChromeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChromeError::NotRunning => write!(f, "chrome_not_running"),
            ChromeError::PermissionDenied => write!(f, "permission_denied"),
            ChromeError::NoTab => write!(f, "no_tab"),
            ChromeError::Script(s) => write!(f, "script_error: {s}"),
        }
    }
}

impl std::error::Error for ChromeError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ChromeTab {
    pub window_index: i64,
    pub tab_index: i64,
    pub url: String,
    pub title: String,
}

fn osascript(script: &str) -> Result<String, ChromeError> {
    osascript_with_args(script, &[])
}

/// Run a script with `on run argv` arguments. Anything user-controlled (URLs, titles)
/// travels through argv so it can never change the script's meaning.
fn osascript_with_args(script: &str, args: &[String]) -> Result<String, ChromeError> {
    let out = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .arg("--")
        .args(args)
        .output()
        .map_err(|e| ChromeError::Script(e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string())
    } else {
        Err(map_osascript_error(&String::from_utf8_lossy(&out.stderr)))
    }
}

/// Map osascript stderr to a typed error.
pub fn map_osascript_error(stderr: &str) -> ChromeError {
    let s = stderr.trim();
    if s.contains("-1743") || s.contains("Not authorized") || s.contains("not allowed assistive") {
        ChromeError::PermissionDenied
    } else if s.contains("-600") || s.contains("isn’t running") || s.contains("isn't running") {
        ChromeError::NotRunning
    } else if s.contains("-1728")
        || s.contains("Can’t get")
        || s.contains("Can't get")
        || s.contains("Invalid index")
    {
        ChromeError::NoTab
    } else {
        ChromeError::Script(s.to_string())
    }
}

/// `pgrep` instead of asking System Events, which would need its own Automation consent.
pub fn is_running() -> Result<bool, ChromeError> {
    is_running_with(Path::new("/usr/bin/pgrep"))
}

/// `is_running` with the `pgrep` binary at `pgrep`; a missing or failing binary is a
/// `Script` error, never a panic.
pub fn is_running_with(pgrep: &Path) -> Result<bool, ChromeError> {
    let status = Command::new(pgrep)
        .args(["-x", "Google Chrome"])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| ChromeError::Script(format!("pgrep: {e}")))?;
    pgrep_result(status.code())
}

/// pgrep exits 0 when a process matched, 1 when none did, and >1 on error.
pub fn pgrep_result(code: Option<i32>) -> Result<bool, ChromeError> {
    match code {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        other => Err(ChromeError::Script(format!("pgrep exited with {other:?}"))),
    }
}

fn ensure_running() -> Result<(), ChromeError> {
    if is_running()? {
        Ok(())
    } else {
        Err(ChromeError::NotRunning)
    }
}

pub fn front_tab() -> Result<ChromeTab, ChromeError> {
    ensure_running()?;
    let out = osascript(
        r#"tell application "Google Chrome"
  if (count of windows) is 0 then error "Can't get window 1" number -1728
  set w to front window
  set t to active tab of w
  return (URL of t) & linefeed & (title of t) & linefeed & (active tab index of w)
end tell"#,
    )?;
    let mut lines = out.splitn(3, '\n');
    let url = lines.next().unwrap_or_default().to_string();
    let title = lines.next().unwrap_or_default().to_string();
    let tab_index = lines
        .next()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    if url.is_empty() {
        return Err(ChromeError::NoTab);
    }
    Ok(ChromeTab {
        window_index: 1,
        tab_index,
        url,
        title,
    })
}

/// Close the front tab, but only if it still shows `expected_url` (when given), so a tab
/// the user switched to while the capture panel was open is never closed by mistake.
/// Returns whether a tab was closed.
pub fn close_front_tab(expected_url: Option<&str>) -> Result<bool, ChromeError> {
    ensure_running()?;
    let out = match expected_url {
        None => osascript(
            r#"tell application "Google Chrome"
  close active tab of front window
end tell
return "closed""#,
        )?,
        Some(url) => osascript_with_args(
            r#"on run argv
  tell application "Google Chrome"
    set t to active tab of front window
    if (URL of t) is equal to (item 1 of argv) then
      close t
      return "closed"
    end if
  end tell
  return "skipped"
end run"#,
            &[url.to_string()],
        )?,
    };
    Ok(parse_close_result(&out))
}

/// The close scripts print `closed` or `skipped`.
pub fn parse_close_result(out: &str) -> bool {
    out.trim() == "closed"
}

/// Every tab in every window, as `window\ttab\turl\ttitle` lines.
pub fn all_tabs() -> Result<Vec<ChromeTab>, ChromeError> {
    ensure_running()?;
    let out = osascript(
        r#"set out to ""
tell application "Google Chrome"
  set wi to 0
  repeat with w in windows
    set wi to wi + 1
    set ti to 0
    repeat with t in tabs of w
      set ti to ti + 1
      set out to out & wi & tab & ti & tab & (URL of t) & tab & (title of t) & linefeed
    end repeat
  end repeat
end tell
return out"#,
    )?;
    Ok(parse_all_tabs_output(&out))
}

pub fn parse_all_tabs_output(out: &str) -> Vec<ChromeTab> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut p = l.splitn(4, '\t');
            let window_index = p.next()?.trim().parse().ok()?;
            let tab_index = p.next()?.trim().parse().ok()?;
            let url = p.next()?.to_string();
            let title = p.next().unwrap_or_default().to_string();
            Some(ChromeTab {
                window_index,
                tab_index,
                url,
                title,
            })
        })
        .collect()
}

/// Script + argv for closing tabs: argv is `w, t, url` triples, highest index first so
/// earlier indexes stay valid, and each tab is closed only if its URL still matches.
pub fn build_close_script(tabs: &[ChromeTab]) -> (String, Vec<String>) {
    let mut sorted: Vec<&ChromeTab> = tabs.iter().collect();
    sorted.sort_by_key(|t| std::cmp::Reverse((t.window_index, t.tab_index)));
    let args = sorted
        .iter()
        .flat_map(|t| {
            [
                t.window_index.to_string(),
                t.tab_index.to_string(),
                t.url.clone(),
            ]
        })
        .collect();
    let script = r#"on run argv
  tell application "Google Chrome"
    set n to count of argv
    set i to 1
    repeat while i + 2 <= n
      set w to (item i of argv) as integer
      set t to (item (i + 1) of argv) as integer
      set u to item (i + 2) of argv
      try
        if (URL of tab t of window w) is equal to u then close tab t of window w
      end try
      set i to i + 3
    end repeat
  end tell
end run"#
        .to_string();
    (script, args)
}

/// Close the given tabs if they still show the URLs they had when listed.
pub fn close_tabs(tabs: &[ChromeTab]) -> Result<(), ChromeError> {
    if tabs.is_empty() {
        return Ok(());
    }
    ensure_running()?;
    let (script, args) = build_close_script(tabs);
    osascript_with_args(&script, &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_osascript_error_cases() {
        assert_eq!(
            map_osascript_error(
                "execution error: Not authorized to send Apple events to Google Chrome. (-1743)"
            ),
            ChromeError::PermissionDenied
        );
        assert_eq!(
            map_osascript_error(
                "execution error: Google Chrome got an error: Can’t get window 1. (-1728)"
            ),
            ChromeError::NoTab
        );
        assert_eq!(
            map_osascript_error("execution error: Application isn’t running. (-600)"),
            ChromeError::NotRunning
        );
        assert!(
            matches!(map_osascript_error("syntax error: weird"), ChromeError::Script(s) if s.contains("weird"))
        );
    }

    #[test]
    fn parse_all_tabs_output_handles_tabs_in_titles() {
        let out = "1\t1\thttps://a.com/\tA site\n1\t2\thttps://b.com/x\tTitle\twith tab\n2\t1\thttps://c.com/\t\n\n";
        let tabs = parse_all_tabs_output(out);
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[1].title, "Title\twith tab");
        assert_eq!(
            tabs[2],
            ChromeTab {
                window_index: 2,
                tab_index: 1,
                url: "https://c.com/".into(),
                title: String::new()
            }
        );
    }

    #[test]
    fn close_script_never_embeds_urls_and_checks_them() {
        let tabs = vec![
            ChromeTab {
                window_index: 1,
                tab_index: 2,
                url: "https://a.com/x?q=\"quote\"".into(),
                title: "A".into(),
            },
            ChromeTab {
                window_index: 1,
                tab_index: 1,
                url: "https://b.com/".into(),
                title: "B".into(),
            },
        ];
        let (script, args) = build_close_script(&tabs);
        assert!(
            !script.contains("a.com") && !script.contains("b.com"),
            "urls must travel as argv, not script text: {script}"
        );
        assert!(script.contains("on run argv"), "{script}");
        assert!(
            script.contains("URL of tab"),
            "must compare the live URL before closing: {script}"
        );
        // Highest index first so earlier indexes stay valid.
        assert_eq!(
            args,
            vec![
                "1",
                "2",
                "https://a.com/x?q=\"quote\"",
                "1",
                "1",
                "https://b.com/"
            ]
        );
    }

    #[test]
    fn pgrep_exit_codes() {
        assert_eq!(pgrep_result(Some(0)), Ok(true));
        assert_eq!(pgrep_result(Some(1)), Ok(false));
        assert!(matches!(pgrep_result(Some(2)), Err(ChromeError::Script(_))));
        assert!(matches!(pgrep_result(None), Err(ChromeError::Script(_))));
    }

    #[test]
    fn missing_pgrep_is_a_script_error() {
        let r = is_running_with(Path::new("/nonexistent/stow-test/pgrep"));
        assert!(
            matches!(r, Err(ChromeError::Script(ref s)) if s.starts_with("pgrep: ")),
            "{r:?}"
        );
        let code: crate::error::CmdError = r.unwrap_err().into();
        assert_eq!(code.code, "script_error");
    }

    #[test]
    fn close_result_parsing() {
        assert!(parse_close_result("closed\n"));
        assert!(!parse_close_result("skipped"));
        assert!(!parse_close_result(""));
    }

    #[test]
    fn error_display_codes() {
        assert_eq!(ChromeError::NotRunning.to_string(), "chrome_not_running");
        assert_eq!(
            ChromeError::PermissionDenied.to_string(),
            "permission_denied"
        );
    }
}
