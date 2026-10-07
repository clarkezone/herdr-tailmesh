#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Viewer,
    Fullscreen,
    Preview(usize),
    Configure(Option<usize>),
}

impl Mode {
    pub fn passive(self) -> bool {
        matches!(self, Self::Fullscreen | Self::Preview(_))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub port: u16,
    pub tree: bool,
    pub check: bool,
    pub help: bool,
    pub mode: Mode,
}

fn handle(value: &str) -> Result<usize, String> {
    let parsed = if let Some(hex) = value.strip_prefix("0x").or(value.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed
        .ok()
        .filter(|h| *h != 0 && *h <= isize::MAX as usize)
        .ok_or_else(|| "Preview/configuration requires a nonzero valid HWND".into())
}

pub fn parse(
    args: impl IntoIterator<Item = String>,
    windows: bool,
    scr: bool,
) -> Result<Options, String> {
    let mut args = args.into_iter().peekable();
    let mut options = Options {
        port: 8790,
        tree: false,
        check: false,
        help: false,
        mode: if scr && windows {
            Mode::Configure(None)
        } else {
            Mode::Viewer
        },
    };
    let mut explicit_mode = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                options.port = args
                    .next()
                    .ok_or("--port needs a value")?
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p != 0)
                    .ok_or("--port must be 1..65535")?;
            }
            "--check" => options.check = true,
            "--tree" => options.tree = true,
            "--help" | "-h" => options.help = true,
            _ => {
                let lower = arg.to_ascii_lowercase();
                let (switch, inline) = lower
                    .split_once(':')
                    .map_or((lower.as_str(), None), |(switch, value)| {
                        (switch, Some(value))
                    });
                if !matches!(switch, "/s" | "-s" | "/p" | "-p" | "/c" | "-c") {
                    return Err(format!("Unknown option: {arg}"));
                }
                if !windows {
                    return Err("Screensaver launch modes are Windows-only".into());
                }
                if explicit_mode {
                    return Err("Choose only one screensaver launch mode".into());
                }
                explicit_mode = true;
                options.mode = match switch {
                    "/s" | "-s" if inline.is_none() => Mode::Fullscreen,
                    "/s" | "-s" => return Err("/s does not accept a HWND".into()),
                    "/p" | "-p" => {
                        let value = inline
                            .map(str::to_owned)
                            .or_else(|| args.next())
                            .ok_or("/p needs a parent HWND")?;
                        Mode::Preview(handle(&value)?)
                    }
                    _ => {
                        let value = inline.map(str::to_owned).or_else(|| {
                            args.peek().filter(|value| {
                                !value.starts_with('-') && !value.starts_with('/')
                            })?;
                            args.next()
                        });
                        Mode::Configure(value.as_deref().map(handle).transpose()?)
                    }
                };
            }
        }
    }
    if options.check && explicit_mode {
        return Err("--check cannot be combined with a screensaver launch mode".into());
    }
    // Preserve headless checks even when invoked through a .scr filename.
    if options.check {
        options.mode = Mode::Viewer;
    }
    Ok(options)
}

pub const INPUT_GRACE_SECONDS: f64 = 1.;

#[derive(Default)]
#[cfg(any(target_os = "windows", test))]
pub struct PointerDismissal {
    baseline: Option<(f64, f64)>,
}

#[cfg(any(target_os = "windows", test))]
impl PointerDismissal {
    pub fn moved(&mut self, elapsed: f64, position: (f64, f64)) -> bool {
        if elapsed < INPUT_GRACE_SECONDS || self.baseline.is_none() {
            self.baseline = Some(position);
            return false;
        }
        let (x, y) = self.baseline.unwrap();
        (position.0 - x).hypot(position.1 - y) >= 8.
    }
}

pub fn automatic_scroll(seconds: f64, maximum: f32) -> f32 {
    if maximum <= 0. {
        return 0.;
    }
    let travel = f64::from(maximum) / 18.;
    let phase = seconds % (2. * travel + 8.);
    let position = if phase < 4. {
        0.
    } else if phase < travel + 4. {
        (phase - 4.) * 18.
    } else if phase < travel + 8. {
        f64::from(maximum)
    } else {
        f64::from(maximum) - (phase - travel - 8.) * 18.
    };
    (position as f32).clamp(0., maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(args: &[&str], windows: bool, scr: bool) -> Result<Options, String> {
        parse(args.iter().map(|s| s.to_string()), windows, scr)
    }
    #[test]
    fn viewer_cli_is_preserved() {
        let o = options(&["--port", "8791", "--check"], false, false).unwrap();
        assert_eq!((o.port, o.check, o.mode), (8791, true, Mode::Viewer));
        assert_eq!(options(&[], true, false).unwrap().mode, Mode::Viewer);
        assert_eq!(options(&[], false, true).unwrap().mode, Mode::Viewer);
        for args in [
            &["--port", "0"][..],
            &["--port", "65536"],
            &["--port"],
            &["unknown"],
        ] {
            assert!(options(args, true, false).is_err());
        }
    }
    #[test]
    fn windows_launch_conventions() {
        assert_eq!(
            options(&[], true, true).unwrap().mode,
            Mode::Configure(None)
        );
        assert_eq!(options(&["/S"], true, true).unwrap().mode, Mode::Fullscreen);
        for args in [&["/P", "123"][..], &["/p:123"], &["-p", "0x7b"]] {
            assert_eq!(options(args, true, true).unwrap().mode, Mode::Preview(123));
        }
        for args in [&["/C", "123"][..], &["/c:123"]] {
            assert_eq!(
                options(args, true, true).unwrap().mode,
                Mode::Configure(Some(123))
            );
        }
        assert_eq!(
            options(&["/c", "--port", "8791"], true, true).unwrap().port,
            8791
        );
        assert_eq!(
            options(&["--check"], true, true).unwrap().mode,
            Mode::Viewer
        );
        for args in [
            &["/p"][..],
            &["/p:0"],
            &["/p:-1"],
            &["/p:garbage"],
            &["/c:0"],
            &["/s:123"],
            &["/s", "/p:123"],
            &["/s", "--check"],
        ] {
            assert!(options(args, true, true).is_err(), "{args:?}");
        }
        for args in [&["/s"][..], &["/p:123"], &["/c"]] {
            assert!(options(args, false, false).is_err());
        }
    }
    #[test]
    fn startup_pointer_noise_and_small_moves_do_not_dismiss() {
        let mut guard = PointerDismissal::default();
        assert!(!guard.moved(0., (0., 0.)));
        assert!(!guard.moved(0.9, (100., 100.)));
        assert!(!guard.moved(1.1, (104., 104.)));
        assert!(guard.moved(1.2, (108., 100.)));
        let mut guard = PointerDismissal::default();
        assert!(!guard.moved(2., (100., 100.)));
        assert!(guard.moved(3., (100., 108.)));
    }
    #[test]
    fn scroll_traverses_pauses_and_returns_within_bounds() {
        assert_eq!(automatic_scroll(100., 0.), 0.);
        assert_eq!(automatic_scroll(3., 180.), 0.);
        assert_eq!(automatic_scroll(9., 180.), 90.);
        assert_eq!(automatic_scroll(15., 180.), 180.);
        assert_eq!(automatic_scroll(23., 180.), 90.);
        assert_eq!(automatic_scroll(28., 180.), 0.);
        for i in 0..1000 {
            assert!((0. ..=180.).contains(&automatic_scroll(f64::from(i) * 0.1, 180.)));
        }
    }
}

#[cfg(test)]
mod orb_launch_tests {
    use super::*;
    #[test]
    fn orb_is_default_and_tree_is_explicit_for_both_launchers() {
        for (args, windows, scr) in [
            (vec![], false, false),
            (vec![], true, false),
            (vec!["/s"], true, true),
            (vec!["/p:123"], true, true),
        ] {
            let options = parse(args.iter().map(|s| s.to_string()), windows, scr).unwrap();
            assert!(!options.tree);
            let legacy = parse(
                args.iter()
                    .map(|s| s.to_string())
                    .chain(Some("--tree".into())),
                windows,
                scr,
            )
            .unwrap();
            assert!(legacy.tree);
            assert_eq!(legacy.mode, options.mode);
            assert_eq!(legacy.port, options.port);
        }
        assert!(
            parse(["--tree".into(), "--check".into()], false, false)
                .unwrap()
                .check
        );
    }
}
