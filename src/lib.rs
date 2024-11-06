use std::os::fd::RawFd;
use std::{error::Error, fmt};
use std::io::{self, IsTerminal};
use std::io::BufRead;
use fdwatcher::FdEvent;
use libc::pid_t;
use nix::sys::wait;
use std::cell::RefCell;
use std::rc::Rc;
use utils::PipeFd;
use std::ffi::CString;
use nix::sched;
use nix::unistd::{dup2, execvp, close};
use std::collections::HashMap;
use twox_hash;


mod fdwatcher;
mod utils;
pub mod signals;

use crate::utils::{Colorize, Color, make_pipe};
pub use crate::utils::{debug_hosts, monotonic_time_ms, generate_seed};
use crate::fdwatcher::PipeType;
pub use crate::fdwatcher::Fdwatcher;



pub const PROG_NAME: &str = "sshp4ru";
const PROG_FULL_NAME: &str = "Parallel SSH Executor in Rust";
pub const PROG_VERSION: &str = "0.1.0";
const PROG_SOURCE: &str = "https://github.com/sshp";
const PROG_LICENSE: &str = "MIT License";

// max characters to process in line and join mode respectively
const DEFAULT_MAX_LINE_LENGTH: u16 = 1 * 1024;
const DEFAULT_MAX_OUTPUT_LENGTH: u16 = 8 * 1024;
const DEFAULT_MAX_SSH_JOBS: u8 = 50;
const _POSIX_HOST_NAME_MAX : usize = 255;

const FDW_MAX_EVENTS: usize = 50; 
const FDW_WAIT_TIMEOUT: i32 = -1; // block indefinitely while waiting for events

const MAX_ARGS: usize = 256;




#[derive(Debug)]
pub enum ParseError {
    UnknownOption,
    HelpRequested,
    VersionRequested,
    ArgCount,
    InvalidColor(String),
    InvalidMaxJobs,
    MaxLineLength,
    MaxOutputLength,
    GroupJoinConflict,
    AnonJoinConflict,
    JoinSilentConflict,
    IoError(io::Error),
    ParsePortError,
    HostnameTooLong(u16, u16, String),
    Utf8Error(std::str::Utf8Error),
    HostFileFormatError(u16, String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseError::UnknownOption => Ok(()),
            ParseError::HelpRequested => Ok(()),
            ParseError::VersionRequested => Ok(()),
            ParseError::ArgCount => write!(f, "no command specified"),
            ParseError::InvalidColor(msg) => write!(f, "invalid value for `-c`: {}", msg),
            ParseError::InvalidMaxJobs => write!(f, "invalid value for `-m`: must be an integer > 0"),
            ParseError::MaxLineLength => write!(f, "invalid value for `--max-line-length`: must be an integer > 0"),
            ParseError::MaxOutputLength => write!(f, "invalid value for `--max-output-length`: must be an integer > 0"),
            ParseError::GroupJoinConflict => write!(f, "`-g` and `-j` are mutually exclusive"),
            ParseError::AnonJoinConflict => write!(f, "`-a` and `-j` are mutually exclusive"),
            ParseError::JoinSilentConflict => write!(f, "`-j` and `-s` are mutually exclusive"),
            ParseError::IoError(err) => write!(f, "{}", err),
            ParseError::ParsePortError => write!(f, "invalid value for `-p`: must be an integer > 0"),
            ParseError::HostnameTooLong(line_no, max_len, msg) => write!(f, "hosts file line {} too long (>= {} chars)\n{}", line_no, max_len, msg),
            ParseError::Utf8Error(err) => write!(f, "{}", err),
            ParseError::HostFileFormatError(line_no, msg) => write!(f, "Host file format error on line: {}\n{}\nEnsure each host is newline separated", line_no, msg),
        }
    }
}

impl Error for ParseError {}

impl From<io::Error> for ParseError {
    fn from(err: io::Error) -> Self {
        ParseError::IoError(err)
    }
}
impl From<ParseError> for io::Error {
    fn from(err: ParseError) -> Self {
        io::Error::new(io::ErrorKind::Other, err)
    }
}

impl From<std::str::Utf8Error> for ParseError {
    fn from(err: std::str::Utf8Error) -> Self {
        ParseError::Utf8Error(err)
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    SshCommandLengthExceeded(usize),
    ClosePipeError(String),
    PipeCreationError(String),
    CloneProcessError,
    TrimError,
    MonitorFdError(String),
    EpollWaitError(io::Error),
    ReadFdError(nix::errno::Errno),
    CloseFdError(nix::errno::Errno),
    WriteStreamError,
    WaitChildProcError(nix::Error),
}
impl Error for RuntimeError {}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RuntimeError::SshCommandLengthExceeded(len) => write!(f, "ssh command exceeds max args: {} >= {}", len, MAX_ARGS),
            RuntimeError::ClosePipeError(pipe_type) => write!(f, "failed to close {} pipe write end", pipe_type),
            RuntimeError::PipeCreationError(pipe_type) => write!(f, "failed to create {} pipe", pipe_type),
            RuntimeError::CloneProcessError => write!(f, "failed to clone process"),
            RuntimeError::TrimError => write!(f, "failed to get the first part of the host name."),
            RuntimeError::MonitorFdError(event) => write!(f,"failed during epoll_ctl system call({}).", event),
            RuntimeError::EpollWaitError(error) => write!(f, "failed during epoll_wait system call: {}", error),
            RuntimeError::ReadFdError(e) => write!(f, "failed to read from file descriptor: {}", e),
            RuntimeError::CloseFdError(e) => write!(f, "failed to close file descriptor: {}", e),
            RuntimeError::WriteStreamError => write!(f, "stream write failed"),
            RuntimeError::WaitChildProcError(e) => write!(f, "failed to wait for child process(waitpid): {}", e),
        }
    }
}



#[derive(Debug,Clone)]
pub enum ProgMode {
    Line = 0,
    Group,
    Join
}

#[derive(Debug)]
enum ScriptInput {
    Stdin(io::Stdin),
    HostsFile(String)
}


#[derive(Debug, Clone)]
pub enum CpState {
    Ready = 0,
    Running,
    Done
}

#[derive(Debug)]
struct ChildProcess{
    pid: pid_t, 
    stdout_fd: i32,
    stderr_fd: i32,
    stdio_fd: i32,
    output_buffer: String,
    output_index: i32,
    exit_code: i32,
    started_time: u128,
    finished_time: u128,
    state: CpState 
}

impl ChildProcess {
    fn new() -> ChildProcess {
        ChildProcess {
            pid: -1,
            stdout_fd: -1,
            stderr_fd: -1,
            stdio_fd: -1,
            output_buffer: String::new(),
            output_index: -1,
            exit_code: -1,
            started_time: 0,
            finished_time: 0,
            state: CpState::Ready
        }
    }
}


#[derive(Debug)]
pub struct Host {
    name: String,
    cp: Box<ChildProcess> // Box or Value
}


impl Host {
    pub fn as_str(&self) -> &str {
        self.name.as_str()
    }
    //public ?
    pub fn cp_exit_code(&self) -> i32 {
        self.cp.exit_code
    }

    pub fn cp_status(&self) -> CpState{
        self.cp.state.clone()
    }
    
    pub fn cp_pid(&self) -> pid_t {
        self.cp.pid
    }
    
    pub fn hostname<'a> (&'a self) -> &'a String {
        &self.name
    }
    
    fn spawn_child_process(&mut self, command: &str, mode: &ProgMode) -> Result<(), RuntimeError>  {
        let mut stdio_fd_pair = PipeFd::default();
        let mut stdout_fd_pair = PipeFd::default();
        let mut stderr_fd_pair = PipeFd::default();
        
        // pipe creation
        match mode {
            ProgMode::Join => { 
                stdio_fd_pair = match make_pipe() {
                    Ok(p) => p,
                    Err(_) => {
                        return Err(RuntimeError::PipeCreationError("stdio".to_string()));                        
                    }
                };
            },
            _ => {
                stdout_fd_pair = match make_pipe() {
                    Ok(p) => p,
                    Err(_) => {
                        return Err(RuntimeError::PipeCreationError("stdout".to_string()));                        
                    }
                };
                stderr_fd_pair = match make_pipe() {
                    Ok(p) => p,
                    Err(_) => {
                        return Err(RuntimeError::PipeCreationError("stderr".to_string()));                        
                    }
                };
                
            }
        }

        if let ProgMode::Join = mode {
            assert_ne!(stdio_fd_pair, stdout_fd_pair);
        } else {
            assert_ne!(stderr_fd_pair, stdio_fd_pair);
            assert_ne!(stdout_fd_pair, stdio_fd_pair);
        }
        
        let mut child_stack = vec![0u8; 8 * 1024 * 1024];
        let ssh_command: Vec<CString> = command.split_whitespace()
                                .map(|s| CString::new(s).unwrap())
                                .collect();
        // println!("ssh command: {:?}", ssh_command);
        // println!("original command {:?}", command);
        match unsafe { 
            sched::clone(
            // Box::new(|| child_process()),
            Box::new( || {
                
                match mode {
                    ProgMode::Join => {
                        // unwrap is safe here in both cases
                        if let Err(e) = dup2(stdio_fd_pair.pipe_write_end.unwrap(), 1) {
                            eprintln!("dup2 stdout error: {}", e);
                            std::process::exit(3);
                        }
                        if let Err(e) = dup2(stdio_fd_pair.pipe_write_end.unwrap(), 2) {
                            eprintln!("dup2 stderr error: {}", e);
                            std::process::exit(3);
                        }
                    },
                    _ => {
                        // newprocess 1> stdout-captured pipe's write end 
                        if let Err(e) = dup2(stdout_fd_pair.pipe_write_end.unwrap(), 1) {
                            eprintln!("dup2 stdout error: {}", e);
                            std::process::exit(3);
                        }
                        // newprocess 2> stderr-captured pipe's write end 
                        if let Err(e) = dup2(stderr_fd_pair.pipe_write_end.unwrap(), 2) {
                            eprintln!("dup2 stderr error: {}", e);
                            std::process::exit(3);
                        }
                    }
                }
                // replace binary with ssh command
                let _ = execvp(&ssh_command[0], &ssh_command);
                eprintln!("exec");
                std::process::exit(3);
                
            }),
            child_stack.as_mut_slice(),
            sched::CloneFlags::CLONE_FS | sched::CloneFlags::CLONE_IO,
            None
            ) 
        } // unsafe block end 
        {
            Ok(pid) => {
                if let ProgMode::Join = mode {
                    if let Err(_) = close(stdio_fd_pair.pipe_write_end.unwrap()) {
                        return Err(RuntimeError::ClosePipeError("stdio".to_string()));
                    }
                    self.cp.stdio_fd = stdio_fd_pair.pipe_read_end.unwrap();
                } 
                else {
                    if let Err(_) = close(stdout_fd_pair.pipe_write_end.unwrap()) {
                        return Err(RuntimeError::ClosePipeError("stdout".to_string()));
                    }
                    
                    if let Err(_) = close(stderr_fd_pair.pipe_write_end.unwrap()) {
                        return Err(RuntimeError::ClosePipeError("stderr".to_string()));
                    }
                    
                    self.cp.stdout_fd = stdout_fd_pair.pipe_read_end.unwrap();
                    self.cp.stderr_fd = stderr_fd_pair.pipe_read_end.unwrap();

                }

                
                self.cp.pid = pid.as_raw();
                self.cp.started_time = monotonic_time_ms();
                self.cp.state = CpState::Running;            
                
                Ok(())
            },
            
            Err(_) => {
                return Err(RuntimeError::CloneProcessError);
            }
        }
    }

    
    fn wait_child_process(&mut self, newline_print: &mut bool, config_params: impl FnOnce() -> (bool, bool, bool)) -> Result<(), RuntimeError> {

        let (debug_opts, exit_codes, colorize) = config_params();
        
        
      
        if let wait::WaitStatus::Exited(pid, exit_code) = wait::waitpid(Some(nix::unistd::Pid::from_raw(self.cp.pid)), 
                                                                              Some(wait::WaitPidFlag::empty()))
                                                                                .map_err(|e| RuntimeError::WaitChildProcError(e))? 
        {
            self.cp.pid = -2;
            self.cp.state = CpState::Done;
            self.cp.exit_code = exit_code;
            self.cp.finished_time = monotonic_time_ms();

            if debug_opts || exit_codes {

                let (magenta, cyan) = if colorize { (Color::Magenta, Color::Cyan)} else {(Color::Empty, Color::Empty)};

                let code_color = if ! colorize { Color::Empty }
                else if self.cp.exit_code == 0 {
                    Color::Green
                } else {
                    Color::Red
                };
            
                let delta = self.cp.finished_time - self.cp.started_time;

           
                if ! *newline_print {
                    print!("\n");
                    *newline_print = true;
                }   

                if debug_opts {
                    print!(
                        "[{}] {} {} exited: {} ",
                        PROG_NAME.colorize(&cyan),
                        pid.to_string().as_str().colorize(&magenta),
                        self.name.as_str().colorize(&cyan),
                        self.cp.exit_code.to_string().as_str().colorize(&code_color)
                    );
                } else {
                    print!(
                        "[{}] exited: {} ",
                        self.name.as_str().colorize(&cyan),
                        self.cp.exit_code.to_string().as_str().colorize(&code_color)
                    );
                }

                println!("({} ms)", delta.to_string().as_str().colorize(&magenta));
            }
        }
        
        
        Ok(())
     
    }

    fn register_cp_fd(&self, mode: &ProgMode, watcher: &Fdwatcher ) -> Result<(), RuntimeError> {
        

        match *mode {
            ProgMode::Join => {
                if let Err(_) = watcher.add(self.cp.stdio_fd) {
                    return Err(RuntimeError::MonitorFdError("EPOLL_CTL_ADD".to_string()));
                }
            },
            _ => {
                if let Err(_) = watcher.add(self.cp.stdout_fd) {
                    return Err(RuntimeError::MonitorFdError("EPOLL_CTL_ADD".to_string()));
                }
                if let Err(_) = watcher.add(self.cp.stderr_fd) {
                    return Err(RuntimeError::MonitorFdError("EPOLL_CTL_ADD".to_string()));
                }
                
            }
        }
        Ok(())

       
    }

}




#[derive(Debug)]
struct SshOpts {
    identity: Option<String>,
    login: Option<String>,
    quiet: bool,
    port: Option<u16>,
    options: Vec<String>
}


impl SshOpts{
    fn build_ssh_command(&self, host: &Host, remote_command: &[String]) -> Result<String, RuntimeError> {
        // base ssh command part
        let mut ssh_command = String::from("ssh");
        
        
        if let Some(id) = &self.identity {
            ssh_command.push_str(&format!(" -i {}", id));              
        }
        if let Some(login) = &self.login {
            ssh_command.push_str(&format!(" -l {}", login));
        }
        
        if let Some(port) = self.port {
            ssh_command.push_str(&format!(" -p {}", port));
        }
        if self.quiet {
            ssh_command.push_str(" -q");
        }
        if self.options.len() > 0 {
            ssh_command.push_str(" -o");
            for opt in self.options.iter() {
                ssh_command.push_str(&format!(" {}", opt));
            }
        }
        
        ssh_command.push_str(format!(" {} ", host.as_str()).as_str());

        // remote command part
        for opt in remote_command.iter() {
            ssh_command.push_str(&format!(" {}", opt));
        }

        if ssh_command.len() >= MAX_ARGS {
            return Err(RuntimeError::SshCommandLengthExceeded(ssh_command.len()));
        }
        // println!("ssh command built: {}", ssh_command);
        Ok(ssh_command)
    
    }
}

impl Default for SshOpts {
    fn default() -> SshOpts {
        SshOpts {
            identity: None,
            login: None,
            quiet: false,
            port: None,
            options: Vec::new()
        }
    }
}


// #[derive(Debug)]
pub struct Config {
    anonymous: bool,
    color: String,
    debug: bool,
    exit_codes: bool,
    file: ScriptInput,
    group: bool,
    join: bool,
    max_jobs: u8,
    dry_run: bool,
    silent: bool,
    trim: bool, 
    exec_path: Option<String>,
    max_line_length: u16,
    max_output_length: u16,

    // SSH user options
    ssh_options: SshOpts,
    //base_ssh_command
    remote_command: Vec<String>,
    mode: ProgMode
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let proc_id = std::process::id().to_string();
        let colorize = self.color == "auto" || self.color == "on";
        let (cyan, green) = if colorize {
            (Color::Cyan, Color::Green)
        } else {
            (Color::Empty, Color::Empty)
        };

        write!(f,"[{}] ssh command: [ {}{}{} ", PROG_NAME.colorize(&cyan), "'".colorize(&green), "ssh".colorize(&green), 
        "'".colorize(&green))?;
        if let Some(id) = &self.ssh_options.identity {
            write!(f,"{}", format!("{}{}{} {}{}{} ", "'".colorize(&green), "-i".colorize(&green), "'".colorize(&green),
            "'".colorize(&green), id.as_str().colorize(&green), "'".colorize(&green)))?;
        }
        if let Some(login) = &self.ssh_options.login {
            write!(f,"{}", format!("{}{}{} {}{}{} ", "'".colorize(&green), "-l".colorize(&green), "'".colorize(&green), 
            "'".colorize(&green), login.as_str().colorize(&green), "'".colorize(&green)))?;
        }
        if let Some(port) = &self.ssh_options.port {
            write!(f,"{}", format!("{}{}{} {}{}{} ", "'".colorize(&green), "-p".colorize(&green), "'".colorize(&green),
            "'".colorize(&green), port.to_string().as_str().colorize(&green), "'".colorize(&green)))?;
        }
        if self.ssh_options.quiet {
            write!(f,"{}", format!("{}{}{} ","'".colorize(&green), "-q".colorize(&green),"'".colorize(&green)))?;
        }
        for opt in self.ssh_options.options.iter() {
            print!("{}{}{} {}{}{} ", "'".colorize(&green), "-o".colorize(&green), "'".colorize(&green), 
            "'".colorize(&green), opt.as_str().colorize(&green), "'".colorize(&green));
        }
        writeln!(f,"]")?;
        
        write!(f,"[{}] remote command: [ ", PROG_NAME.colorize(&cyan))?;
        for arg in self.remote_command.iter() {
            write!(f,"{} ", format!("{}{}{}", "'".colorize(&green), arg.as_str().colorize(&green), "'".colorize(&green)))?;
        }
        writeln!(f,"]")?;

        writeln!(f,"[{}] pid: {}", PROG_NAME.colorize(&cyan), proc_id.as_str().colorize(&green))?;
        writeln!(f,"[{}] mode: {}", PROG_NAME.colorize(&cyan), self.mode().colorize(&green))?;
        write!(f, "[{}] max-jobs: {}", PROG_NAME.colorize(&cyan), self.max_jobs.to_string().as_str().colorize(&green))
    }
}

impl Config {
    pub fn new(args: &[String]) -> Result<Config, ParseError> {
        let mut config = Config::default();
        let mut help_opt = false;
        let mut unknown_opt = false;

        let mut cnt = 0;
        while cnt < args.len(){
            let arg = args.get(cnt).unwrap();
            if ! (arg.starts_with("-") || arg.starts_with("--")){
                        break;
            }
        
            match arg.as_str() {
                "-a" | "--anonymous"=> config.anonymous = true,
                "-d" | "--debug" => config.debug = true,
                "-e" | "--exit-codes" => config.exit_codes = true,
                "-g" | "--group" => config.group = true,
                "-j" | "--join" => config.join = true,
                "-n" | "--dry-run" => config.dry_run = true,
                "-q" | "--quiet" => config.ssh_options.quiet = true,
                "-s" | "--silent" => config.silent = true,
                "-t" | "--trim" => config.trim = true,
                "-m" | "--max-jobs" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(max_jobs) => config.max_jobs = max_jobs.parse().unwrap_or(0),
                        None => {
                            // actual argument not provided
                            config.max_jobs = 0;
                            cnt -= 1;
                        }
                    }
                },
                "--max-line-length" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(max_line_length) => config.max_line_length = max_line_length.parse().unwrap_or(0),
                        None => {
                            // actual argument not provided
                            config.max_line_length = 0;
                            cnt -= 1;
                        }
                    }
                },
                "--max-output-length" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(max_output_length) => config.max_output_length = max_output_length.parse().unwrap_or(0),
                        None => {
                            config.max_output_length = 0;
                            cnt -= 1;
                        }
                    }
                },
                "-p" | "--port" => {
                    cnt += 1;
                    match args.get(cnt) {
                        Some(port_str) => {
                            config.ssh_options.port = match port_str.parse::<u16>() {
                                Ok(port) => Some(port),  
                                Err(_) => return Err(ParseError::ParsePortError)
                            };
                        },
                        None => {
                            //cnt -= 1;  
                            return Err(ParseError::ParsePortError);
                        }
                    }
                }, 
                "-c" | "--color" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(color) => config.color = String::from(color),
                        None => {
                            config.color = "".to_string();
                            cnt -= 1;
                        }
                    }
                },
                "-l" | "--login" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(login) => config.ssh_options.login = Some(String::from(login)),
                        None => {
                            config.ssh_options.login = None;
                            cnt -= 1;
                        }
                    }
                },
                "-i" | "--identity" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(identity) => {

                            if let Some(next_arg) = args.get(cnt) {
                                if next_arg == "-" {
                                    config.ssh_options.identity = Some(String::from("-"));
                                }
                                else {
                                    config.ssh_options.identity = Some(String::from(identity))
                                }
                            }
                        },
                        None => {
                            config.ssh_options.identity = Some("".to_string());
                            cnt -= 1;
                        }
                    }
                },
                "-f" | "--file" => {
                    cnt += 1;
                    match args.get(cnt){
                        Some(file) => {
                            
                            if let Some(next_arg) = args.get(cnt) {
                                if next_arg == "-" {
                                     config.file = ScriptInput::Stdin(io::stdin());
                                }
                                else {
                                    config.file = ScriptInput::HostsFile(file.clone());
                                }
                            }
                            
                        },
                        None => {
                            config.file = ScriptInput::HostsFile("".to_string());
                            cnt -= 1;
                        }
                    }
                },
                "-o" | "--option" => {
                    cnt += 1;
                    match args.get(cnt) {
                        Some(option) => config.ssh_options.options.push(option.clone()),
                        None => {
                            config.ssh_options.options.push("".to_string());
                            cnt -= 1;
                        }
                    }
                },
                "-x" | "--exec" => {
                    cnt += 1;
                    match args.get(cnt) {
                        Some(exec_path) => config.exec_path = Some(exec_path.clone()),
                        None => {
                            config.exec_path = Some("".to_string());
                            cnt -= 1;
                        }
                    }
                }
                "-v" | "--version" => {
                    return Err(ParseError::VersionRequested);
                },
                "-h" | "--help" => help_opt = true,
                _ => unknown_opt = true,            

            } // end of match
            cnt += 1;
        } // end of while loop

        if args.len() < 1 {
            return Err(ParseError::ArgCount);
        }

        if config.anonymous && config.join {
            return Err(ParseError::AnonJoinConflict);
        }

        if config.group && config.join {
            return Err(ParseError::GroupJoinConflict);
        }

        if config.join && config.silent {
            return Err(ParseError::JoinSilentConflict);
        }

        if config.max_jobs == 0 {
            return Err(ParseError::InvalidMaxJobs)
        }

        if config.max_line_length == 0 {
            return Err(ParseError::MaxLineLength);
        }

        if config.max_output_length == 0 {
            return Err(ParseError::MaxOutputLength);
        }
        
        assert!(!(config.join && config.group));
        if config.join {
            config.mode = ProgMode::Join;
        }
        else if config.group {
            config.mode = ProgMode::Group;
        }

        if ! ["auto", "on", "off"].contains(&config.color.as_str())  {
            return Err(ParseError::InvalidColor(config.color));
        }
        else if config.color == "auto".to_string() || config.color == "on".to_string() {
            let stdout = io::stdout();  
            if ! stdout.is_terminal() { 
                config.color = "off".to_string();
            }
        } 
        else {
            config.color = "off".to_string();
            
        }

        if help_opt {
            utils::print_usage(io::stdout(), &config.color)?;
            return Err(ParseError::HelpRequested);
        }
        
        if unknown_opt {
            utils::print_usage(io::stderr(), &config.color)?;
            return Err(ParseError::UnknownOption);
        }
        
        config.remote_command = args[cnt..].to_vec();

        Ok(config)
    }

    pub fn parse_hosts(&self) -> Result<Vec<Rc<RefCell<Host>>>, ParseError> {

        let bad_chars = ['\n', ' ', '\0', '#']; 
        let begins_with_bad_char = |s: &str| -> bool {
            s.starts_with(&bad_chars[..])
        };
        let mut line_no = 0;

        let process_line = |line: &str, line_no: u32, hosts: &mut Vec<Rc<RefCell<Host>>>| -> Result<(), ParseError> {
            if !begins_with_bad_char(&line) && line.ends_with("\n"){
                if line.chars().count() >= _POSIX_HOST_NAME_MAX {
                    return Err(ParseError::HostnameTooLong(line_no as u16, _POSIX_HOST_NAME_MAX as u16, line.to_string()));
                }
                let cp = Box::new(ChildProcess::new());
                hosts.push( Rc::new(RefCell::new(Host { name: line.trim().to_string(), cp })) );
            }
            else if !line.ends_with("\n") && !begins_with_bad_char(&line){
                return Err(ParseError::HostFileFormatError(line_no as u16, line.to_string()));
            }
            Ok(())
        };
        
        match &self.file {
            ScriptInput::HostsFile(file) => {
                // transform error to custom error type
                let file = std::fs::File::open(file).map_err(ParseError::IoError)?;
                let mut reader = io::BufReader::new(file);
                let mut hosts: Vec<Rc<RefCell<Host>>> = Vec::new();
                let mut buffer:Vec<u8> = Vec::new();

                while reader.read_until(b'\n', &mut buffer)? > 0 {
                    line_no += 1;
                    let line = std::str::from_utf8(&buffer)?;
                    process_line(line, line_no, &mut hosts)?;
                    buffer.clear();
                    
                }
                
                Ok(hosts)
            },
            ScriptInput::Stdin(stdin) => {
                if stdin.is_terminal() {
                    return Err(ParseError::IoError(io::Error::new(io::ErrorKind::Other, "No hosts provided from stdin!")));
                }
                // buffered reads on locked stdin
                let mut reader = io::BufReader::new(stdin.lock());
                let mut hosts: Vec<Rc<RefCell<Host>>> = Vec::new();
                let mut buffer:Vec<u8> = Vec::new();
                
                while reader.read_until(b'\n', &mut buffer)? > 0 {
                    line_no += 1;
                    let line = std::str::from_utf8(&buffer)?;
                    process_line(line, line_no, &mut hosts)?;
                    buffer.clear();   
                }
                Ok(hosts)
            }   
        }
    }

    pub fn debugging(&self) -> bool {
        self.debug
    }
    pub fn color(&self) -> &str {
        self.color.as_str()
    }
    pub fn mode (&self) -> &str {
        match self.mode {
            ProgMode::Line => "LINE",
            ProgMode::Group => "GROUP",
            ProgMode::Join => "JOIN"
        }
    }
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
   
}

impl Default for Config {
    fn default() -> Config {
        Config {
            anonymous: false,
            color: "auto".to_string(),
            debug: false,
            exit_codes: false,
            file: ScriptInput::Stdin(io::stdin()),
            group: false,
            join: false,
            max_jobs: DEFAULT_MAX_SSH_JOBS,
            dry_run: false,
            silent: false,
            trim: false,
            exec_path: None,
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
            max_output_length: DEFAULT_MAX_OUTPUT_LENGTH,
            ssh_options: Default::default(),
            remote_command: Vec::new(),
            mode: ProgMode::Line
        }
    }
}


fn finish_join_mode(hosts: &mut Vec<Rc<RefCell<Host>>>, colorize: bool) {

    let num_hosts = hosts.len();
    let seed = generate_seed();
    let mut unique_hosts = 0;
    let mut hosts_map: HashMap<u64, (u32,Vec<Rc<RefCell<Host>>>)> = HashMap::new();
    let (magenta, cyan) = if colorize {(Color::Magenta, Color::Cyan)} else {(Color::Empty, Color::Empty)};
    
    for h in hosts.iter(){
        let mut host = h.borrow_mut();
        if host.cp.output_index >= 0 {
            continue;
        }
        let hash = twox_hash::XxHash64::oneshot(seed, host.cp.output_buffer.as_bytes());
        if hosts_map.contains_key(&hash) {
            hosts_map.get_mut(&hash).unwrap().0 += 1;
            hosts_map.get_mut(&hash).unwrap().1.push(Rc::clone(&h));
            host.cp.output_index = unique_hosts;
        } else {
            hosts_map.insert(hash, (1, vec![Rc::clone(&h)]));
            unique_hosts += 1;
        }   
    }

    println!("finished with {} unique result{}\n", unique_hosts.to_string().as_str().colorize(&magenta), 
            if unique_hosts == 1 { "" } else { "s" });
    
    for (_, (num_same, grouped_hosts)) in hosts_map.iter() {
        print!("hosts ({}/{}):", num_same.to_string().as_str().colorize(&magenta), 
                num_hosts.to_string().as_str().colorize(&magenta));

        for host in grouped_hosts.iter(){
            let host = host.borrow();
            print!(" {}", host.name.as_str().colorize(&cyan));
          
        }
        
        // grouped_hosts vector has always at least one element
        let last_host = grouped_hosts.last().unwrap().borrow();
        
        if last_host.cp.output_buffer.is_empty() {
            print!("{}", "- no output -".colorize(&magenta));
        } 
        else {
            print!("\n{}", last_host.cp.output_buffer);
            if ! last_host.cp.output_buffer.ends_with('\n') {
                println!();
            }
        }
        println!();
    }
        

}

pub fn run(conf: &Config, hosts: &mut Vec<Rc<RefCell<Host>>>, fdwatcher: &mut Fdwatcher) -> Result<(), RuntimeError>{
    let mut done: u16 = 0;
    let mut remaining  = 0;

    let colorize = conf.color == "auto" || conf.color == "on";
    let (cyan, magenta) = if colorize {
         (Color::Cyan, Color::Magenta)
     } else {
         (Color::Empty, Color::Empty)
     };

    //only for group mode
    let mut newline_group_print = true;

    let mut events_map: HashMap<i32, FdEvent> = if conf.mode() == "JOIN" 
    { HashMap::with_capacity(hosts.len()) } else { HashMap::with_capacity(hosts.len() * 2) };

    
    if conf.mode() == "JOIN" && io::stdout().is_terminal() {
       print!("[{}] finished {}/{}\r", PROG_NAME.colorize(&cyan), 
                done.to_string().as_str().colorize(&magenta), 
                hosts.len().to_string().as_str().colorize(&magenta));
    }

    let mut hosts_iter = hosts.iter().peekable();
 
    while hosts_iter.peek().is_some() || remaining > 0 {

        //spawn jobs
        while hosts_iter.peek().is_some() && remaining < conf.max_jobs  {
            let host = hosts_iter.next().unwrap();

            let command = match &conf.exec_path {
                Some(exec_path) => exec_path,
                None => &conf.ssh_options.build_ssh_command(&host.borrow(), &conf.remote_command)?
            };

            //spawn child process            
            host.borrow_mut().spawn_child_process(command.as_str(), &conf.mode)?;
            if conf.debug {
                println!("[{}] {} {} spawned", PROG_NAME.colorize(&cyan) ,host.borrow().cp.pid.to_string().as_str().colorize(&magenta), 
                                            host.borrow().name.as_str().colorize(&cyan));
            }

            //store fd events
            match conf.mode {
                ProgMode::Join => {
                        events_map.insert(host.borrow().cp.stdio_fd, FdEvent::new(Rc::clone(&host), PipeType::StdIO));
                },
                _ => {
                        events_map.insert(host.borrow().cp.stdout_fd, FdEvent::new(Rc::clone(&host), PipeType::StdOut));
                        events_map.insert(host.borrow().cp.stderr_fd, FdEvent::new(Rc::clone(&host), PipeType::StdErr));
                }
            }

            //trim
            if conf.trim {
                let name = host.borrow().name.clone();
                host.borrow_mut().name = name
                            .split('.').nth(0)
                            .ok_or_else(|| RuntimeError::TrimError)?
                            .to_string();
            }
            

            //register fd to epoll
            host.borrow().register_cp_fd(&conf.mode, &fdwatcher)?;

            remaining += 1;
        }        
        
        let mut completed_events: [RawFd; FDW_MAX_EVENTS] = [0; FDW_MAX_EVENTS];
        let num_completed_events = fdwatcher.wait(&mut completed_events, FDW_MAX_EVENTS, FDW_WAIT_TIMEOUT)?;
        
        for event_fd in completed_events[..num_completed_events].iter() {
            
            if let Some(event) = events_map.get_mut(event_fd) {
                
                //last_host is used to stimulate the newline print behavior in group mode
                //without utilizing a static mut global variable
                let mut last_host:Option<String> = None;
                let config_req_params = || -> (bool, ProgMode, u16, u16, bool, bool) {
                    (conf.silent, conf.mode.clone(), conf.max_line_length, conf.max_output_length, conf.anonymous, colorize)
                };
                
            
                // read from the active fd and output if mode is not join, 
                // untill the child process is done writing or it would block
                let data_read = event.read_active_fd(&fdwatcher, &mut last_host, 
                                            &mut newline_group_print, config_req_params)?;
                
                //check if child is done writing and close the pipe.
                let pipe_done: bool = (event.get_host().borrow().cp.stderr_fd == -2 && event.get_host().borrow().cp.stdout_fd == -2) || 
                event.get_host().borrow().cp.stdio_fd == -2;
                
                if data_read && pipe_done {
                    // need to delegate errors
                    let config_wait_params = || -> (bool, bool, bool) {
                        (conf.debug, conf.exit_codes, colorize)
                    };

                    event.get_host().borrow_mut().wait_child_process(&mut newline_group_print, config_wait_params)?;
                    remaining -= 1;
                    done += 1;
               
                    if conf.mode() == "JOIN" && io::stdout().is_terminal() {
                        print!("[{}] finished {}/{}\r", PROG_NAME.colorize(&cyan), 
                                 done.to_string().as_str().colorize(&magenta), 
                                 hosts.len().to_string().as_str().colorize(&magenta));
                        
                        if usize::from(done) == hosts.len() {
                            print!("\n\n");
                        }
                     }
                }
            }
        }        
    }  // main event loop

    if conf.mode() == "JOIN" {
        finish_join_mode(hosts, colorize);
    }

    Ok(())

}

