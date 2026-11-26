//! `rsmigemo`: a command-line driver compatible with `cmigemo`.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rsmigemo::{Dictionary, Migemo, Syntax};

const DICT_FILENAME: &str = "migemo-dict";

#[cfg(windows)]
const DEFAULT_DIRS: &[&str] = &[
    "./dict/mdict/cp932",
    "../dict/mdict/cp932",
    "./build/dict/mdict/cp932",
];
#[cfg(not(windows))]
const DEFAULT_DIRS: &[&str] = &[
    "./dict/mdict/utf-8",
    "../dict/mdict/utf-8",
    "./build/dict/mdict/utf-8",
];
const LEGACY_DIRS: &[&str] = &["./dict", "../dict", "./build/dict"];

#[derive(Default)]
struct Options {
    dict: Option<PathBuf>,
    subdicts: Vec<PathBuf>,
    save: Option<PathBuf>,
    sdict: Option<PathBuf>,
    word: Option<String>,
    vim: bool,
    emacs: bool,
    rust: bool,
    no_newline: bool,
    quiet: bool,
}

fn usage() -> String {
    format!(
        "\
rsmigemo {version} - a Rust port of C/Migemo

USAGE: rsmigemo [OPTIONS]

OPTIONS:
  -d --dict <dict>      Use a file <dict> for dictionary.
  -s --subdict <dict>   Sub dictionary files (repeatable).
  -u --userdict <dict>  Alias for -s/--subdict
     --convert          Accepted for compatibility (always static).
     --save <dict>      Save the dictionary as a static dictionary (sdict).
     --sdict <dict>     Load <dict> as static dictionary (sdict)
                        (conflicts with -d/s/u and --save)
  -q --quiet            Show no message except results.
  -v --vim              Use vim style regexp.
  -e --emacs            Use emacs style regexp.
  -r --rust             Use Rust regex crate style regexp.
  -n --nonewline        Don't use newline match.
  -w --word <word>      Expand a <word> and soon exit.
  -h --help             Show this message.
",
        version = env!("CARGO_PKG_VERSION")
    )
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.to_string_lossy().into_owned();
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("missing argument: {name}"))
        };
        match arg.as_str() {
            "-v" | "--vim" => opts.vim = true,
            "-e" | "--emacs" => opts.emacs = true,
            "-r" | "--rust" => opts.rust = true,
            "-n" | "--nonewline" => opts.no_newline = true,
            "-q" | "--quiet" => opts.quiet = true,
            "--convert" => {}
            "-d" | "--dict" => opts.dict = Some(value(&arg)?.into()),
            "-s" | "--subdict" | "-u" | "--userdict" => opts.subdicts.push(value(&arg)?.into()),
            "--save" => opts.save = Some(value(&arg)?.into()),
            "--sdict" => opts.sdict = Some(value(&arg)?.into()),
            "-w" | "--word" => opts.word = Some(value(&arg)?.to_string_lossy().into_owned()),
            "-h" | "--help" => {
                print!("{}", usage());
                std::process::exit(0);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            // cmigemo ignores these; a forgotten `-w` should not silently
            // start the interactive prompt.
            _ => {
                return Err(format!(
                    "unexpected argument: {arg} (use -w to expand a word)"
                ));
            }
        }
    }
    if opts.sdict.is_some()
        && (opts.dict.is_some() || !opts.subdicts.is_empty() || opts.save.is_some())
    {
        return Err("--sdict conflicts with -d/s/u and --save".into());
    }
    Ok(opts)
}

fn default_dict() -> Option<PathBuf> {
    DEFAULT_DIRS
        .iter()
        .chain(LEGACY_DIRS)
        .map(|dir| Path::new(dir).join(DICT_FILENAME))
        .find(|path| path.is_file())
}

fn open(opts: &Options) -> Result<Migemo, String> {
    // Explicit paths are opened as given, so FIFOs work and the error says
    // why a file could not be read. Only the default search needs a check.
    let main = match opts.sdict.as_ref().or(opts.dict.as_ref()) {
        Some(path) => path.clone(),
        None => default_dict().ok_or("failed to load default dictionaries")?,
    };
    let mut builder = Migemo::builder();
    builder = if opts.sdict.is_some() {
        builder.dictionary(Dictionary::open_sdict(&main).map_err(|e| e.to_string())?)
    } else {
        builder.dictionary_file(&main).map_err(|e| e.to_string())?
    };
    // Same as Migemo::open, with sub-dictionaries merged.
    for sub in &opts.subdicts {
        builder = builder.dictionary_file(sub).map_err(|e| e.to_string())?;
    }
    Ok(builder
        .sibling_tables(&main)
        .map_err(|e| e.to_string())?
        .build())
}

fn syntax(opts: &Options) -> Syntax {
    // With several flags the first of vim, emacs, rust wins, as `-v` beats
    // `-e` in cmigemo.
    let syntax = if opts.vim {
        Syntax::vim()
    } else if opts.emacs {
        Syntax::emacs()
    } else if opts.rust {
        Syntax::rust_regex()
    } else {
        Syntax::cmigemo()
    };
    if opts.no_newline {
        syntax.without_newline()
    } else {
        syntax
    }
}

fn run(opts: Options) -> Result<(), String> {
    let migemo = open(&opts)?;
    if let Some(path) = &opts.save {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        migemo
            .dictionary()
            .write_sdict(std::io::BufWriter::new(file))
            .map_err(|e| format!("failed to save sdict: {e}"))?;
        if opts.word.is_none() && !opts.quiet {
            println!("sdict saved to: {}", path.display());
        }
    }
    let syntax = syntax(&opts);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if let Some(word) = &opts.word {
        let pattern = migemo.query_with(word, &syntax);
        // No newline with -v, as in cmigemo: Vim's `system()` would keep it
        // as part of the pattern.
        let newline = if opts.vim { "" } else { "\n" };
        write!(out, "{pattern}{newline}").map_err(|e| e.to_string())?;
        return Ok(());
    }

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut line = Vec::new();
    loop {
        if !opts.quiet {
            write!(out, "QUERY: ")
                .and_then(|()| out.flush())
                .map_err(|e| e.to_string())?;
        }
        line.clear();
        if input
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            if !opts.quiet {
                writeln!(out).map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        let query = String::from_utf8_lossy(&line);
        let query = query.strip_suffix('\n').unwrap_or(&query);
        let query = query.strip_suffix('\r').unwrap_or(query);
        let pattern = migemo.query_with(query, &syntax);
        let prefix = if opts.quiet { "" } else { "PATTERN: " };
        writeln!(out, "{prefix}{pattern}")
            .and_then(|()| out.flush())
            .map_err(|e| e.to_string())?;
    }
}

fn main() -> ExitCode {
    let result = parse_args().and_then(run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ERROR: {message}");
            ExitCode::FAILURE
        }
    }
}
