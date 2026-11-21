use std::path::PathBuf;

use crate::error::{Result, WinedroidError};

const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5000;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutomationScript {
    pub(crate) steps: Vec<AutomationStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutomationStep {
    pub(crate) line: usize,
    pub(crate) delay_ms: u64,
    pub(crate) action: AutomationAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TapTarget {
    Point { x: u32, y: u32 },
    Text(String),
    ViewId(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AutomationAction {
    Tap {
        target: TapTarget,
        hold_ms: u64,
    },
    Swipe {
        start: (u32, u32),
        end: (u32, u32),
        duration_ms: u64,
        steps: Option<u32>,
    },
    Pointer {
        kind: PointerKind,
        pointer: u32,
        x: u32,
        y: u32,
    },
    Scroll {
        delta_y: i32,
        point: Option<(u32, u32)>,
    },
    Rotary {
        axis_scroll: f32,
    },
    Key {
        name: KeyName,
    },
    Text {
        text: String,
    },
    WaitIdle {
        timeout_ms: u64,
    },
    WaitText {
        text: String,
        timeout_ms: u64,
    },
    Assert {
        kind: AssertKind,
    },
    Screenshot {
        path: Option<PathBuf>,
    },
    Dump {
        kind: DumpKind,
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerKind {
    Down,
    Move,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyName {
    Back,
    Home,
    Menu,
    Enter,
    Del,
    Tab,
    Space,
    VolumeUp,
    VolumeDown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssertKind {
    Text(String),
    NoText(String),
    View(i32),
    Display(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DumpKind {
    Bounds,
    Surface,
    Ui,
}

impl AutomationScript {
    pub(crate) fn parse(contents: &str) -> Result<Self> {
        let mut steps = Vec::new();

        for (index, raw_line) in contents.lines().enumerate() {
            let line = index + 1;
            let without_comment = strip_comment(raw_line);
            let tokens = tokenize(without_comment, line)?;
            if tokens.is_empty() {
                continue;
            }

            steps.push(parse_step(line, &tokens)?);
        }

        Ok(Self { steps })
    }
}

fn strip_comment(line: &str) -> &str {
    match line.split_once('#') {
        Some((before, _)) => before,
        None => line,
    }
}

fn tokenize(line: &str, source_line: usize) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;

    for ch in line.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }

    if in_quotes {
        return Err(WinedroidError::new(format!(
            "automation line {source_line}: unterminated quoted string"
        )));
    }
    if has_token {
        tokens.push(current);
    }

    Ok(tokens)
}

fn parse_step(line: usize, tokens: &[String]) -> Result<AutomationStep> {
    let delay_ms = parse_u64(&tokens[0], line, "delay")?;
    let Some(verb) = tokens.get(1) else {
        return Err(WinedroidError::new(format!(
            "automation line {line}: expected a verb after the delay"
        )));
    };
    let args = &tokens[2..];
    let action = parse_action(line, verb, args)?;

    Ok(AutomationStep {
        line,
        delay_ms,
        action,
    })
}

fn parse_action(line: usize, verb: &str, args: &[String]) -> Result<AutomationAction> {
    match verb {
        "tap" => {
            let (target, hold_token) = match args.first().map(String::as_str) {
                Some("text") => {
                    expect_arg_count(line, verb, args, 2, 3)?;
                    (TapTarget::Text(args[1].clone()), args.get(2))
                }
                Some("id") => {
                    expect_arg_count(line, verb, args, 2, 3)?;
                    (TapTarget::ViewId(args[1].clone()), args.get(2))
                }
                _ => {
                    expect_arg_count(line, verb, args, 1, 2)?;
                    let (x, y) = parse_coord(&args[0], line)?;
                    (TapTarget::Point { x, y }, args.get(1))
                }
            };
            let hold_ms = match hold_token {
                Some(value) => parse_u64(value, line, "hold_ms")?,
                None => 0,
            };
            Ok(AutomationAction::Tap { target, hold_ms })
        }
        "swipe" => {
            expect_arg_count(line, verb, args, 3, 4)?;
            let start = parse_coord(&args[0], line)?;
            let end = parse_coord(&args[1], line)?;
            let duration_ms = parse_u64(&args[2], line, "duration_ms")?;
            let steps = match args.get(3) {
                Some(value) => Some(parse_positive_u32(value, line, "steps")?),
                None => None,
            };
            Ok(AutomationAction::Swipe {
                start,
                end,
                duration_ms,
                steps,
            })
        }
        "down" | "move" | "up" => {
            expect_arg_count(line, verb, args, 1, 2)?;
            let kind = match verb {
                "down" => PointerKind::Down,
                "move" => PointerKind::Move,
                _ => PointerKind::Up,
            };
            let (pointer, coord_token) = if args.len() == 2 {
                (parse_u32(&args[0], line, "pointer")?, &args[1])
            } else {
                (0, &args[0])
            };
            let (x, y) = parse_coord(coord_token, line)?;
            Ok(AutomationAction::Pointer {
                kind,
                pointer,
                x,
                y,
            })
        }
        "scroll" => {
            expect_arg_count(line, verb, args, 1, 2)?;
            let delta_y = parse_i32(&args[0], line, "delta_y")?;
            let point = match args.get(1) {
                Some(value) => Some(parse_coord(value, line)?),
                None => None,
            };
            Ok(AutomationAction::Scroll { delta_y, point })
        }
        "rotary" => {
            expect_arg_count(line, verb, args, 1, 1)?;
            Ok(AutomationAction::Rotary {
                axis_scroll: parse_finite_f32(&args[0], line, "axis_scroll")?,
            })
        }
        "key" => {
            expect_arg_count(line, verb, args, 1, 1)?;
            Ok(AutomationAction::Key {
                name: parse_key_name(&args[0], line)?,
            })
        }
        "text" => {
            expect_arg_count(line, verb, args, 1, 1)?;
            Ok(AutomationAction::Text {
                text: args[0].clone(),
            })
        }
        "waitidle" => {
            expect_arg_count(line, verb, args, 0, 1)?;
            let timeout_ms = match args.first() {
                Some(value) => parse_u64(value, line, "timeout_ms")?,
                None => DEFAULT_WAIT_TIMEOUT_MS,
            };
            Ok(AutomationAction::WaitIdle { timeout_ms })
        }
        "waittext" => {
            expect_arg_count(line, verb, args, 1, 2)?;
            let timeout_ms = match args.get(1) {
                Some(value) => parse_u64(value, line, "timeout_ms")?,
                None => DEFAULT_WAIT_TIMEOUT_MS,
            };
            Ok(AutomationAction::WaitText {
                text: args[0].clone(),
                timeout_ms,
            })
        }
        "assert" => parse_assert(line, args),
        "screenshot" => {
            expect_arg_count(line, verb, args, 0, 1)?;
            Ok(AutomationAction::Screenshot {
                path: args.first().map(PathBuf::from),
            })
        }
        "dump" => {
            expect_arg_count(line, verb, args, 0, 2)?;
            let kind = match args.first().map(String::as_str) {
                None | Some("bounds") => DumpKind::Bounds,
                Some("surface") => DumpKind::Surface,
                Some("ui") => DumpKind::Ui,
                Some(other) => {
                    return Err(WinedroidError::new(format!(
                        "automation line {line}: dump expects `bounds`, `surface`, or `ui`, got `{other}`"
                    )));
                }
            };
            let path = args.get(1).map(PathBuf::from);
            if path.is_some() && kind != DumpKind::Ui {
                return Err(WinedroidError::new(format!(
                    "automation line {line}: only `dump ui` accepts a path argument"
                )));
            }
            Ok(AutomationAction::Dump { kind, path })
        }
        other => Err(WinedroidError::new(format!(
            "automation line {line}: unknown verb `{other}`"
        ))),
    }
}

fn parse_assert(line: usize, args: &[String]) -> Result<AutomationAction> {
    let Some(kind) = args.first() else {
        return Err(WinedroidError::new(format!(
            "automation line {line}: assert expects `text`, `notext`, `view`, or `display`"
        )));
    };

    let kind = match kind.as_str() {
        "text" => {
            expect_arg_count(line, "assert text", &args[1..], 1, 1)?;
            AssertKind::Text(args[1].clone())
        }
        "notext" => {
            expect_arg_count(line, "assert notext", &args[1..], 1, 1)?;
            AssertKind::NoText(args[1].clone())
        }
        "view" => {
            expect_arg_count(line, "assert view", &args[1..], 1, 1)?;
            AssertKind::View(parse_view_id(&args[1], line)?)
        }
        "display" => {
            expect_arg_count(line, "assert display", &args[1..], 1, 1)?;
            AssertKind::Display(args[1].clone())
        }
        other => {
            return Err(WinedroidError::new(format!(
                "automation line {line}: assert expects `text`, `notext`, `view`, or `display`, got `{other}`"
            )));
        }
    };

    Ok(AutomationAction::Assert { kind })
}

fn expect_arg_count(
    line: usize,
    verb: &str,
    args: &[String],
    min: usize,
    max: usize,
) -> Result<()> {
    if args.len() < min || args.len() > max {
        let expected = if min == max {
            format!("{min}")
        } else {
            format!("{min}..{max}")
        };
        return Err(WinedroidError::new(format!(
            "automation line {line}: `{verb}` expects {expected} argument(s), got {}",
            args.len()
        )));
    }

    Ok(())
}

fn parse_coord(token: &str, line: usize) -> Result<(u32, u32)> {
    let Some((x, y)) = token.split_once(':') else {
        return Err(WinedroidError::new(format!(
            "automation line {line}: coordinate must be `X:Y`, got `{token}`"
        )));
    };
    let x = parse_u32(x, line, "x")?;
    let y = parse_u32(y, line, "y")?;
    Ok((x, y))
}

fn parse_u64(token: &str, line: usize, field: &str) -> Result<u64> {
    token.parse::<u64>().map_err(|_| {
        WinedroidError::new(format!(
            "automation line {line}: {field} must be a non-negative integer, got `{token}`"
        ))
    })
}

fn parse_u32(token: &str, line: usize, field: &str) -> Result<u32> {
    token.parse::<u32>().map_err(|_| {
        WinedroidError::new(format!(
            "automation line {line}: {field} must be a non-negative integer, got `{token}`"
        ))
    })
}

fn parse_i32(token: &str, line: usize, field: &str) -> Result<i32> {
    token.parse::<i32>().map_err(|_| {
        WinedroidError::new(format!(
            "automation line {line}: {field} must be an integer, got `{token}`"
        ))
    })
}

fn parse_finite_f32(token: &str, line: usize, field: &str) -> Result<f32> {
    match token.parse::<f32>() {
        Ok(value) if value.is_finite() && value != 0.0 => Ok(value),
        _ => Err(WinedroidError::new(format!(
            "automation line {line}: {field} must be a nonzero number, got `{token}`"
        ))),
    }
}

fn parse_positive_u32(token: &str, line: usize, field: &str) -> Result<u32> {
    let value = parse_u32(token, line, field)?;
    if value == 0 {
        return Err(WinedroidError::new(format!(
            "automation line {line}: {field} must be greater than zero"
        )));
    }
    Ok(value)
}

fn parse_view_id(token: &str, line: usize) -> Result<i32> {
    token.parse::<i32>().map_err(|_| {
        WinedroidError::new(format!(
            "automation line {line}: view id must be an integer, got `{token}`"
        ))
    })
}

fn parse_key_name(token: &str, line: usize) -> Result<KeyName> {
    match token {
        "back" => Ok(KeyName::Back),
        "home" => Ok(KeyName::Home),
        "menu" => Ok(KeyName::Menu),
        "enter" => Ok(KeyName::Enter),
        "del" => Ok(KeyName::Del),
        "tab" => Ok(KeyName::Tab),
        "space" => Ok(KeyName::Space),
        "volup" => Ok(KeyName::VolumeUp),
        "voldown" => Ok(KeyName::VolumeDown),
        other => Err(WinedroidError::new(format!(
            "automation line {line}: unknown key `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(contents: &str) -> AutomationScript {
        AutomationScript::parse(contents).expect("script should parse")
    }

    #[test]
    fn parses_the_spec_example() {
        let script = parse("100 tap 485:283\n2000 swipe 593:920 392:293 200\n");

        assert_eq!(
            script.steps,
            vec![
                AutomationStep {
                    line: 1,
                    delay_ms: 100,
                    action: AutomationAction::Tap {
                        target: TapTarget::Point { x: 485, y: 283 },
                        hold_ms: 0,
                    },
                },
                AutomationStep {
                    line: 2,
                    delay_ms: 2000,
                    action: AutomationAction::Swipe {
                        start: (593, 920),
                        end: (392, 293),
                        duration_ms: 200,
                        steps: None,
                    },
                },
            ]
        );
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let script = parse("# header comment\n\n  \n0 tap 1:2   # trailing comment\n");

        assert_eq!(script.steps.len(), 1);
        assert_eq!(script.steps[0].line, 4);
    }

    #[test]
    fn parses_tap_hold_and_swipe_steps() {
        let script = parse("0 tap 10:20 600\n0 swipe 1:2 3:4 250 12\n");

        assert_eq!(
            script.steps[0].action,
            AutomationAction::Tap {
                target: TapTarget::Point { x: 10, y: 20 },
                hold_ms: 600,
            }
        );
        assert_eq!(
            script.steps[1].action,
            AutomationAction::Swipe {
                start: (1, 2),
                end: (3, 4),
                duration_ms: 250,
                steps: Some(12),
            }
        );
    }

    #[test]
    fn parses_scroll_with_optional_point() {
        let script = parse("0 scroll 120\n0 scroll -48 210:380\n");

        assert_eq!(
            script.steps[0].action,
            AutomationAction::Scroll {
                delta_y: 120,
                point: None,
            }
        );
        assert_eq!(
            script.steps[1].action,
            AutomationAction::Scroll {
                delta_y: -48,
                point: Some((210, 380)),
            }
        );
    }

    #[test]
    fn parses_rotary_axis_and_rejects_zero_or_non_numeric() {
        let script = parse("0 rotary -1.5\n0 rotary 2\n");

        assert_eq!(
            script.steps[0].action,
            AutomationAction::Rotary { axis_scroll: -1.5 }
        );
        assert_eq!(
            script.steps[1].action,
            AutomationAction::Rotary { axis_scroll: 2.0 }
        );
        for invalid in [
            "0 rotary 0",
            "0 rotary up",
            "0 rotary",
            "0 rotary 1 2",
            "0 rotary NaN",
        ] {
            assert!(
                AutomationScript::parse(invalid).is_err(),
                "`{invalid}` should be rejected"
            );
        }
    }

    #[test]
    fn parses_pointer_primitives_with_optional_id() {
        let script = parse("0 down 5:6\n0 move 1 7:8\n0 up 1 7:8\n");

        assert_eq!(
            script.steps[0].action,
            AutomationAction::Pointer {
                kind: PointerKind::Down,
                pointer: 0,
                x: 5,
                y: 6,
            }
        );
        assert_eq!(
            script.steps[1].action,
            AutomationAction::Pointer {
                kind: PointerKind::Move,
                pointer: 1,
                x: 7,
                y: 8,
            }
        );
        assert_eq!(
            script.steps[2].action,
            AutomationAction::Pointer {
                kind: PointerKind::Up,
                pointer: 1,
                x: 7,
                y: 8,
            }
        );
    }

    #[test]
    fn parses_sync_assert_and_io_verbs() {
        let script = parse(
            "0 waitidle\n\
             0 waitidle 1500\n\
             0 waittext \"AC\"\n\
             0 waittext \"Create\" 8000\n\
             0 assert display \"16\"\n\
             0 assert text \"Settings\"\n\
             0 assert notext \"Error\"\n\
             0 assert view 2131296466\n\
             0 key back\n\
             0 text \"hello world\"\n\
             0 screenshot out.png\n\
             0 dump bounds\n",
        );

        assert_eq!(
            script.steps[0].action,
            AutomationAction::WaitIdle { timeout_ms: 5000 }
        );
        assert_eq!(
            script.steps[1].action,
            AutomationAction::WaitIdle { timeout_ms: 1500 }
        );
        assert_eq!(
            script.steps[2].action,
            AutomationAction::WaitText {
                text: "AC".to_owned(),
                timeout_ms: 5000,
            }
        );
        assert_eq!(
            script.steps[3].action,
            AutomationAction::WaitText {
                text: "Create".to_owned(),
                timeout_ms: 8000,
            }
        );
        assert_eq!(
            script.steps[4].action,
            AutomationAction::Assert {
                kind: AssertKind::Display("16".to_owned()),
            }
        );
        assert_eq!(
            script.steps[7].action,
            AutomationAction::Assert {
                kind: AssertKind::View(2131296466),
            }
        );
        assert_eq!(
            script.steps[8].action,
            AutomationAction::Key {
                name: KeyName::Back,
            }
        );
        assert_eq!(
            script.steps[9].action,
            AutomationAction::Text {
                text: "hello world".to_owned(),
            }
        );
        assert_eq!(
            script.steps[10].action,
            AutomationAction::Screenshot {
                path: Some(PathBuf::from("out.png")),
            }
        );
        assert_eq!(
            script.steps[11].action,
            AutomationAction::Dump {
                kind: DumpKind::Bounds,
                path: None,
            }
        );
    }

    #[test]
    fn parses_tap_by_text_and_view_id_targets() {
        let script = parse(
            "0 tap text \"7\"\n\
             0 tap text \"( )\" 600\n\
             0 tap id sevenButton\n\
             0 tap id com.example:id/sevenButton 250\n",
        );

        assert_eq!(
            script.steps[0].action,
            AutomationAction::Tap {
                target: TapTarget::Text("7".to_owned()),
                hold_ms: 0,
            }
        );
        assert_eq!(
            script.steps[1].action,
            AutomationAction::Tap {
                target: TapTarget::Text("( )".to_owned()),
                hold_ms: 600,
            }
        );
        assert_eq!(
            script.steps[2].action,
            AutomationAction::Tap {
                target: TapTarget::ViewId("sevenButton".to_owned()),
                hold_ms: 0,
            }
        );
        assert_eq!(
            script.steps[3].action,
            AutomationAction::Tap {
                target: TapTarget::ViewId("com.example:id/sevenButton".to_owned()),
                hold_ms: 250,
            }
        );
    }

    #[test]
    fn rejects_malformed_steps() {
        for invalid in [
            "tap 1:2",
            "100",
            "100 tap",
            "100 tap 1",
            "100 tap 1:2 3 4",
            "100 wiggle 1:2",
            "100 swipe 1:2 3:4",
            "100 swipe 1:2 3:4 5 0",
            "100 key fly",
            "100 assert color red",
            "-5 tap 1:2",
            "100 text \"open",
        ] {
            assert!(
                AutomationScript::parse(invalid).is_err(),
                "expected `{invalid}` to be rejected"
            );
        }
    }
}
