use std::process::ExitCode;
use std::os::unix::io::AsRawFd;
use nix::unistd::dup2;
use sshp4ru::{debug_hosts, Config, ParseError, PROG_NAME, PROG_VERSION};
use sshp4ru::signals::SignalHandler;
use sshp4ru::RuntimeError;

fn main() -> ExitCode {
    let mut exit_code: ExitCode = ExitCode::SUCCESS;
    let start_time = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    
    let config = Config::new(&args)
                        .unwrap_or_else(|err| {
                            match err {
                                ParseError::HelpRequested => {
                                    std::process::exit(0);
                                },
                                ParseError::VersionRequested => {
                                    println!("{} {}", PROG_NAME, PROG_VERSION);
                                    std::process::exit(0);
                                },
                                ParseError::UnknownOption => {
                                    std::process::exit(2);
                                },
                                _ => {
                                    println!("{}", err);
                                    std::process::exit(2);
                                }
                            }
                        });

    let mut hosts = config.parse_hosts()
                        .unwrap_or_else(|err| {
                            println!("{}", err);
                            std::process::exit(2);
                });

    if hosts.len() < 1 {
        eprintln!("{}: no hosts specified", PROG_NAME);
        std::process::exit(2);
    }

    // 0> /dev/null
    let dev_null = std::fs::File::open("/dev/null").unwrap_or_else(|error| {
        eprintln!("open /dev/null error: {}", error);
        std::process::exit(3);
    });
    dup2(dev_null.as_raw_fd(), 0).unwrap_or_else(|error| {
        eprintln!("open /dev/null error: {}", error);
        std::process::exit(3);
    });


    let mut fdwatcher = sshp4ru::Fdwatcher::new().unwrap_or_else(|error| {
        eprintln!("Fdwatcher creation error: {}", error);
        std::process::exit(3);
    });

    // signals
    let colorize = config.color() == "auto" || config.color() == "on";
    let mut signal_handler = SignalHandler::new(&hosts, hosts.len(), colorize);
    signal_handler.register_signals();
  
    //debugging
    if config.debugging() {
        debug_hosts(&hosts, colorize); 
        println!("{:?}", config);
    }

    if config.dry_run() {
        println!("(dry run)");
    } 
    else {
        sshp4ru::run(&config, &mut hosts, &mut fdwatcher).unwrap_or_else(|err: RuntimeError| {
            match err {
                RuntimeError::SshCommandLengthExceeded(_) | RuntimeError::TrimError => {
                    eprintln!("{}", err);
                    std::process::exit(2);
                },
                _ => {
                    eprintln!("{}", err);
                    std::process::exit(3);
                }
            }
        });

        
        for host in hosts.iter() {
            let child_proc_exit_code = host.borrow().cp_exit_code();
            assert!(child_proc_exit_code >= 0, "main: Assertion `host.cp.exit_code >= 0' failed.");
            if child_proc_exit_code < 0 {
                eprintln!("Error: Child process exit code must be non-negative, got: {}", child_proc_exit_code);
                std::process::exit(1); 
            }
            if child_proc_exit_code != 0 {
                exit_code = ExitCode::from(1);
            }
        }
    }
    
    let delta = start_time.elapsed();
    if config.debugging() {
        let (cyan, reset, magenta) = if colorize {
         ("\x1b[36m", "\x1b[0m", "\x1b[35m")
        } else {
            ("", "" ,"")
        };
        println!("[{}{}{}] finished ({}{:0.5}{} ms)", cyan, PROG_NAME, reset, magenta, delta.as_millis(), reset);
    }


    SignalHandler::unregister_signals();
    return exit_code;
}

