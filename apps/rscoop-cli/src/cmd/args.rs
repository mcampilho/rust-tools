//! `getopt` (`lib\getopt.ps1`): the options of a command.

/// (letter, long name, takes a value)
pub type Spec = (char, &'static str, bool);

#[derive(Debug, Default)]
pub struct Opts {
    found: Vec<(char, String)>,
    pub rest: Vec<String>,
}

impl Opts {
    pub fn has(&self, letter: char) -> bool {
        self.found.iter().any(|(l, _)| *l == letter)
    }

    pub fn value(&self, letter: char) -> Option<&str> {
        self.found.iter().rev().find(|(l, _)| *l == letter).map(|(_, v)| v.as_str())
    }
}

/// The options of `args`; Err is the option that is not known or lacks its
/// value. `--` ends the options.
pub fn getopt(args: &[String], spec: &[Spec]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            o.rest.extend(args.cloned());
            break;
        }
        if let Some(name) = arg.strip_prefix("--") {
            let (letter, _, takes) = spec.iter().find(|(_, long, _)| *long == name).ok_or_else(|| arg.clone())?;
            let value = if *takes { args.next().ok_or_else(|| arg.clone())?.clone() } else { String::new() };
            o.found.push((*letter, value));
        } else if arg.len() > 1 && arg.starts_with('-') {
            let letters: Vec<char> = arg.chars().skip(1).collect();
            for (i, letter) in letters.iter().enumerate() {
                let (_, _, takes) = spec.iter().find(|(l, _, _)| l == letter).ok_or_else(|| format!("-{letter}"))?;
                let value = if *takes {
                    if i + 1 != letters.len() {
                        return Err(format!("-{letter}"));
                    }
                    args.next().ok_or_else(|| format!("-{letter}"))?.clone()
                } else {
                    String::new()
                };
                o.found.push((*letter, value));
            }
        } else {
            o.rest.push(arg.clone());
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_scoop_reads_options() {
        let spec: &[Spec] = &[('g', "global", false), ('k', "cache", false), ('a', "arch", true)];
        let args = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        let o = getopt(&args("-gk x --arch 32bit * -- -z"), spec).unwrap();
        assert!(o.has('g') && o.has('k'));
        assert_eq!(o.value('a'), Some("32bit"));
        assert_eq!(o.rest, ["x", "*", "-z"]);
        for bad in ["-z", "--zz", "-a", "-ag 1", "--arch"] {
            assert!(getopt(&args(bad), spec).is_err(), "{bad}");
        }
    }
}
