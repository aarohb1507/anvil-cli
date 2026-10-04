use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_DIARY: &str = ".anvil/diary.tsv";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedStep {
    input_hash: String,
    output: String,
}

#[derive(Debug, Default)]
struct Diary {
    completed: HashMap<String, CompletedStep>,
}

impl Diary {
    fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let file = File::open(path)?;
        let mut diary = Self::default();

        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some(event) = DiaryEvent::parse(&line) else {
                continue;
            };

            if let DiaryEvent::Complete {
                step_id,
                input_hash,
                output,
            } = event
            {
                diary
                    .completed
                    .entry(step_id)
                    .or_insert(CompletedStep { input_hash, output });
            }
        }

        Ok(diary)
    }

    fn result_for(&self, step_id: &str, input_hash: &str) -> Option<&str> {
        self.completed
            .get(step_id)
            .filter(|entry| entry.input_hash == input_hash)
            .map(|entry| entry.output.as_str())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DiaryEvent {
    Started {
        step_id: String,
        input_hash: String,
    },
    Complete {
        step_id: String,
        input_hash: String,
        output: String,
    },
    Failed {
        step_id: String,
        input_hash: String,
        code: String,
    },
}

impl DiaryEvent {
    fn parse(line: &str) -> Option<Self> {
        let mut parts = line.splitn(5, '\t');
        let _timestamp = parts.next()?;
        let kind = parts.next()?;
        let step_id = decode_field(parts.next()?).ok()?;
        let input_hash = parts.next()?.to_string();
        let value = parts.next().unwrap_or_default();

        match kind {
            "started" => Some(Self::Started {
                step_id,
                input_hash,
            }),
            "complete" => Some(Self::Complete {
                step_id,
                input_hash,
                output: decode_field(value).ok()?,
            }),
            "failed" => Some(Self::Failed {
                step_id,
                input_hash,
                code: value.to_string(),
            }),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("anvil: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("run") => run_step(&args[1..]),
        Some("status") => show_status(&args[1..]),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown command `{command}`. Try `anvil help`.")),
    }
}

fn run_step(args: &[String]) -> Result<(), String> {
    let mut diary_path = PathBuf::from(DEFAULT_DIARY);
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--diary" => {
                index += 1;
                diary_path = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or("--diary needs a path")?;
                index += 1;
            }
            "--" => break,
            _ => break,
        }
    }

    let step_id = args.get(index).ok_or("missing step id")?;
    let input = args.get(index + 1).ok_or("missing input text")?;
    let separator = args.get(index + 2).map(String::as_str);
    if separator != Some("--") {
        return Err("expected `--` before the command to run".into());
    }

    let command = args
        .get(index + 3)
        .ok_or("missing command after `--`")?
        .to_string();
    let command_args = &args[index + 4..];
    let input_hash = hash_input(input);

    let diary = Diary::load(&diary_path).map_err(|error| error.to_string())?;
    if let Some(output) = diary.result_for(step_id, &input_hash) {
        print!("{output}");
        return Ok(());
    }

    append_event(
        &diary_path,
        &DiaryEvent::Started {
            step_id: step_id.clone(),
            input_hash: input_hash.clone(),
        },
    )?;

    let output = Command::new(command)
        .args(command_args)
        .output()
        .map_err(|error| format!("failed to start command: {error}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        append_event(
            &diary_path,
            &DiaryEvent::Complete {
                step_id: step_id.clone(),
                input_hash,
                output: stdout.clone(),
            },
        )?;
        print!("{stdout}");
        Ok(())
    } else {
        let code = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string());
        append_event(
            &diary_path,
            &DiaryEvent::Failed {
                step_id: step_id.clone(),
                input_hash,
                code: code.clone(),
            },
        )?;
        Err(format!("wrapped command failed with exit code {code}"))
    }
}

fn show_status(args: &[String]) -> Result<(), String> {
    let diary_path = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DIARY));
    let diary = Diary::load(&diary_path).map_err(|error| error.to_string())?;

    println!("completed_steps={}", diary.completed.len());
    for step_id in diary.completed.keys() {
        println!("{step_id}");
    }

    Ok(())
}

fn append_event(path: &Path, event: &DiaryEvent) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;

    writeln!(file, "{}", format_event(event)).map_err(|error| error.to_string())
}

fn format_event(event: &DiaryEvent) -> String {
    let timestamp = unix_timestamp();
    match event {
        DiaryEvent::Started {
            step_id,
            input_hash,
        } => {
            format!("{timestamp}\tstarted\t{}\t{input_hash}\t", encode_field(step_id))
        }
        DiaryEvent::Complete {
            step_id,
            input_hash,
            output,
        } => format!(
            "{timestamp}\tcomplete\t{}\t{input_hash}\t{}",
            encode_field(step_id),
            encode_field(output)
        ),
        DiaryEvent::Failed {
            step_id,
            input_hash,
            code,
        } => {
            format!("{timestamp}\tfailed\t{}\t{input_hash}\t{code}", encode_field(step_id))
        }
    }
}

fn hash_input(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;

    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{hash:016x}")
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn encode_field(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
}

fn decode_field(value: &str) -> Result<String, String> {
    let mut decoded = String::new();
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            decoded.push(ch);
            continue;
        }

        let code = format!(
            "{}{}",
            chars.next().ok_or("truncated escape")?,
            chars.next().ok_or("truncated escape")?
        );
        match code.as_str() {
            "25" => decoded.push('%'),
            "09" => decoded.push('\t'),
            "0A" => decoded.push('\n'),
            _ => return Err(format!("unknown escape %{code}")),
        }
    }

    Ok(decoded)
}

fn print_help() {
    println!(
        "anvil - durable step runner\n\n\
Usage:\n  \
anvil run [--diary PATH] <step-id> <input> -- <command> [args...]\n  \
anvil status [PATH]\n\n\
Example:\n  \
anvil run summarize:page-80 page-80.txt -- sh -c 'echo summary text'\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_completed_events_and_returns_matching_result() {
        let path = temp_path("completed");
        fs::write(
            &path,
            format!(
                "1\tcomplete\t{}\tabc\t{}\n",
                encode_field("step-1"),
                encode_field("first\n")
            ),
        )
        .unwrap();

        let diary = Diary::load(&path).unwrap();

        assert_eq!(diary.result_for("step-1", "abc"), Some("first\n"));
        assert_eq!(diary.result_for("step-1", "different"), None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn keeps_first_completed_result_for_a_step() {
        let path = temp_path("first-result");
        fs::write(
            &path,
            "1\tcomplete\tstep-1\tabc\tfirst\n2\tcomplete\tstep-1\tabc\tsecond\n",
        )
        .unwrap();

        let diary = Diary::load(&path).unwrap();

        assert_eq!(diary.result_for("step-1", "abc"), Some("first"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn encodes_tabs_newlines_and_percent_signs() {
        let original = "a\tb\n100%";

        assert_eq!(decode_field(&encode_field(original)).unwrap(), original);
    }

    fn temp_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!("anvil-{name}-{}.tsv", std::process::id()))
    }
}
