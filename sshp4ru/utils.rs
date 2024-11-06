use crate::Host;
use crate::{PROG_FULL_NAME, PROG_LICENSE, PROG_NAME, PROG_SOURCE, PROG_VERSION};
use chrono::prelude::*;
use nix::fcntl::OFlag;
use nix::unistd::pipe2;
use rand::rngs::OsRng;
use rand::Rng;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;
use std::{
    io::{self, Write},
    os::fd::{IntoRawFd, RawFd},
};

#[allow(unused)]
pub enum Color {
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    Reset,
    White,
    Yellow,
    Empty,
}

impl Color {
    pub fn as_str(&self) -> &'static str {
        match self {
            Color::Black => "\x1b[030m",
            Color::Blue => "\x1b[034m",
            Color::Cyan => "\x1b[036m",
            Color::Green => "\x1b[032m",
            Color::Magenta => "\x1b[035m",
            Color::Red => "\x1b[031m",
            Color::Reset => "\x1b[0m",
            Color::White => "\x1b[037m",
            Color::Yellow => "\x1b[033m",
            Color::Empty => "",
        }
    }
}

pub trait Colorize {
    fn colorize(&self, col: &Color) -> String;
}

impl Colorize for &str {
    fn colorize(&self, col: &Color) -> String {
        if let Color::Empty = col {
            return self.to_string();
        }
        format!("{}{}{}", col.as_str(), self, Color::Reset.as_str())
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct PipeFd {
    pub pipe_read_end: Option<RawFd>,
    pub pipe_write_end: Option<RawFd>,
}

impl Default for PipeFd {
    fn default() -> Self {
        PipeFd {
            pipe_read_end: None,
            pipe_write_end: None,
        }
    }
}

pub fn make_pipe() -> Result<PipeFd, nix::Error> {
    let (pipe_read_end, pipe_write_end) = pipe2(OFlag::O_NONBLOCK | OFlag::O_CLOEXEC)?;
    Ok(PipeFd {
        pipe_read_end: Some(pipe_read_end.into_raw_fd()),
        pipe_write_end: Some(pipe_write_end.into_raw_fd()),
    })
}

pub fn print_usage<T: Write>(out: T, c: &str) -> io::Result<()> {
    let mut handle = io::BufWriter::new(out);
    let datetime = Local::now();
    let date = datetime.format("%Y-%m-%d").to_string();
    let time = datetime.format("%H:%M:%S").to_string();
    let fdwatcher_interface = if cfg!(feature = "USE_KQUEUE") {
        "kqueue"
    } else {
        "epoll"
    };

    let colorize = |s: &str, col: &Color| -> String {
        if c == "auto" || c == "on" {
            s.colorize(col)
        } else {
            s.colorize(&Color::Empty)
        }
    };
    let green = Color::Green;
    let magenta = Color::Magenta;
    let yellow = Color::Yellow;
    //             _           _  _
    //     ___ ___| |__  _ __ | || |  _ __ _   _
    //    / __/ __| '_ \| '_ \| || |_| '__| | | |
    //    \__ \__ \ | | | |_) |__   _| |  | |_| |
    //    |___/___/_| |_| .__/   |_| |_|   \__,_|
    //                  |_|

    writeln!(
        handle,
        "          {}           {}  {}              ",
        colorize("_", &magenta),
        colorize("_", &magenta),
        colorize("_", &magenta)
    )?;
    write!(
        handle,
        "  {}   ",
        colorize("___ ___| |__  _ __ | || |  _ __ _   _ ", &magenta)
    )?;
    writeln!(
        handle,
        "  {} ({})",
        colorize(PROG_FULL_NAME, &green),
        colorize(PROG_VERSION, &green)
    )?;
    write!(
        handle,
        " {}   ",
        colorize("/ __/ __| '_ \\| '_ \\| || |_| '__| | | |", &magenta)
    )?;
    writeln!(
        handle,
        "  {} {}",
        colorize("Source:", &green),
        colorize(PROG_SOURCE, &green)
    )?;
    write!(
        handle,
        " {}  ",
        colorize("\\__ \\__ \\ | | | |_) |__   _| |  | |_| |", &magenta)
    )?;
    writeln!(
        handle,
        "   {} {} {} (using {})",
        colorize("Compiled:", &green),
        colorize(date.as_str(), &green),
        colorize(time.as_str(), &green),
        colorize(fdwatcher_interface, &green)
    )?;
    write!(
        handle,
        " {}",
        colorize("|___/___/_| |_| .__/   |_| |_|   \\__,_|", &magenta)
    )?;
    writeln!(handle, "     {}", colorize(PROG_LICENSE, &green))?;
    writeln!(handle, "               {}      ", colorize("|_|", &magenta))?;
    writeln!(handle)?; // Empty line

    writeln!(handle, "Parallel ssh with streaming output.")?;
    writeln!(handle)?; // Empty line

    // Usage
    writeln!(handle, "{}", colorize("USAGE:", &yellow))?;
    writeln!(
        handle,
        "    {1} {0}",
        colorize("[-m maxjobs] [-f file] command ...", &green),
        colorize(PROG_NAME, &green)
    )?;
    writeln!(handle)?; // Empty line

    // Examples
    writeln!(handle, "{}", colorize("EXAMPLES:", &yellow))?;
    writeln!(
        handle,
        "    ssh into a list of hosts passed via stdin and get the output of {}.\n",
        colorize("uname -v", &green)
    )?;
    writeln!(
        handle,
        "      {1} {0}",
        colorize("uname -v < hosts", &green),
        colorize(PROG_NAME, &green)
    )?;
    writeln!(handle)?; // Empty line

    writeln!(
        handle,
        "    ssh into a list of hosts passed on the command line, limit max parallel
    connections to 3, and grab the output of {}.\n",
        colorize("pgrep", &green)
    )?;
    writeln!(
        handle,
        "      {1} {0}",
        colorize("-m 3 -f hosts.txt pgrep -fl process", &green),
        colorize(PROG_NAME, &green)
    )?;
    writeln!(handle)?; // Empty line

    writeln!(
        handle,
        "    Upgrade packages on all hosts in the list one-by-one, grouping the output
    by host, with debugging output enabled.\n"
    )?;
    writeln!(
        handle,
        "      {1} {0}",
        colorize("-m 1 -f hosts.txt -d -g pkg-manager update", &green),
        colorize(PROG_NAME, &green)
    )?;
    writeln!(handle)?; // Empty line

    // Options
    writeln!(handle, "{}", colorize("OPTIONS:", &yellow))?;
    write!(
        handle,
        "  {}, {}",
        colorize("-a", &green),
        colorize("--anonymous", &green)
    )?;
    writeln!(
        handle,
        "\t     Hide hostname prefix, defaults to {}.",
        colorize("false", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-c", &green),
        colorize("--color <on|off|auto>", &green)
    )?;
    writeln!(
        handle,
        "  Set color output, defaults to {}.",
        colorize("auto", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-d", &green),
        colorize("--debug", &green)
    )?;
    writeln!(
        handle,
        "\t             Enable debug info, defaults to {}.",
        colorize("false", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-e", &green),
        colorize("--exit-codes", &green)
    )?;
    writeln!(
        handle,
        "\t     Show command exit codes, defaults to {}.",
        colorize("false", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-f", &green),
        colorize("--file <file>", &green)
    )?;
    writeln!(
        handle,
        "\t     A file of hosts separated by newlines, defaults to {}.",
        colorize("stdin", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-g", &green),
        colorize("--group", &green)
    )?;
    writeln!(
        handle,
        "\t             Group output by hostname ({}).",
        colorize("group mode", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-h", &green),
        colorize("--help", &green)
    )?;
    writeln!(handle, "\t             Print this message and exit.")?;
    write!(
        handle,
        "  {}, {}",
        colorize("-j", &green),
        colorize("--join", &green)
    )?;
    writeln!(
        handle,
        "\t             Join hosts together by output ({}).",
        colorize("join mode", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-m", &green),
        colorize("--max-jobs <num>", &green)
    )?;
    writeln!(
        handle,
        "\t     Max processes to run concurrently, defaults to {}.",
        colorize("50", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-n", &green),
        colorize("--dry-run", &green)
    )?;
    writeln!(
        handle,
        "\t             Don't actually execute subprocesses."
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-s", &green),
        colorize("--silent", &green)
    )?;
    writeln!(
        handle,
        "\t             Silence all output subprocess stdio, defaults to {}.",
        colorize("false", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-t", &green),
        colorize("--trim", &green)
    )?;
    writeln!(
        handle,
        "\t             Trim hostnames (remove domain) on output, defaults to {}.",
        colorize("false", &green)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-v", &green),
        colorize("--version", &green)
    )?;
    writeln!(handle, "\t             Print the version number and exit.")?;
    write!(
        handle,
        "  {}, {}",
        colorize("-x", &green),
        colorize("--exec <prog>", &green)
    )?;
    writeln!(
        handle,
        "          Program to execute, defaults to {}.",
        colorize("ssh", &green)
    )?;
    write!(handle, "  {} ", colorize("--max-line-length <num>", &green))?;
    writeln!(
        handle,
        "   Maximum line length (in line mode), defaults to {}.",
        colorize("1024", &green)
    )?;
    write!(
        handle,
        "  {} ",
        colorize("--max-output-length <num>", &green)
    )?;
    writeln!(
        handle,
        " Maximum output length (in join mode), defaults to {}.",
        colorize("8192", &green)
    )?;
    writeln!(handle)?; // Empty line

    // SSH options
    writeln!(
        handle,
        "{} (passed directly to ssh)",
        colorize("SSH OPTIONS:", &yellow)
    )?;
    write!(
        handle,
        "  {}, {}",
        colorize("-i", &green),
        colorize("--identity <ident>", &green)
    )?;
    writeln!(handle, "     ssh identity file to use.")?;
    write!(
        handle,
        "  {}, {}",
        colorize("-l", &green),
        colorize("--login <name>", &green)
    )?;
    writeln!(handle, "         The username to login as.")?;
    write!(
        handle,
        "  {}, {}",
        colorize("-o", &green),
        colorize("--option <key=val>", &green)
    )?;
    writeln!(handle, "     ssh option passed in key=value form.")?;
    write!(
        handle,
        "  {}, {}",
        colorize("-p", &green),
        colorize("--port <port>", &green)
    )?;
    writeln!(handle, "          The ssh port.")?;
    write!(
        handle,
        "  {}, {}",
        colorize("-q", &green),
        colorize("--quiet", &green)
    )?;
    writeln!(handle, "                Run ssh in quiet mode.")?;
    writeln!(handle)?; // Empty line

    // More
    writeln!(handle, "{}", colorize("MORE:", &yellow))?;
    writeln!(
        handle,
        "    See {}(1) for more information.",
        colorize("sshp", &green)
    )?;
    Ok(())
}

pub fn debug_hosts(hosts: &Vec<Rc<RefCell<Host>>>, colorize: bool) -> () {
    let host_count: &str = &hosts.len().to_string();

    let (cyan, magenta, green) = if colorize {
        (Color::Cyan, Color::Magenta, Color::Green)
    } else {
        (Color::Empty, Color::Empty, Color::Empty)
    };
    print!(
        "[{}] hosts ({}): [ ",
        PROG_NAME.colorize(&cyan),
        host_count.colorize(&magenta)
    );
    for host in hosts {
        print!(
            "{} ",
            format!(
                "{}{}{}",
                "'".colorize(&green),
                host.borrow().as_str().colorize(&green),
                "'".colorize(&green)
            )
        );
    }
    println!("]");
}

pub fn monotonic_time_ms() -> u128 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    now.as_millis()
}

pub fn generate_seed() -> u64 {
    OsRng.gen()
}
