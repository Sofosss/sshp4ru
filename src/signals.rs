use libc::sigprocmask;
use libc::{sigaction, sigemptyset, SIG_BLOCK, SIGINT, SIGUSR1, SIGTERM, SA_RESTART};
use std::cell::RefCell;
use std::ptr;
use std::rc::Rc;
use crate::Host;
use crate::CpState;
use crate::utils::{Colorize, Color}; 


static mut PROGRAM_CONTEXT: *const Vec<Rc<RefCell<Host>>> = ptr::null_mut();
static mut HOSTS_LEN: usize =  0;
static mut COLORIZE: bool = false;

pub struct SignalHandler {
    sigint: libc::sigaction,
    sigusr1: libc::sigaction,
    sigkill: libc::sigaction,

    hosts_context: *const Vec<Rc<RefCell<Host>>>,
    hosts_len: usize,
    colorize: bool
}

impl SignalHandler{
    pub fn new(program_ctx: *const Vec<Rc<RefCell<Host>>>, hosts_len: usize, colorize: bool) -> SignalHandler {
        SignalHandler {
            sigint: sigaction {
                sa_sigaction: handle_sigint_term as usize,
                sa_flags: SA_RESTART,
                sa_restorer: None,
                ..unsafe { std::mem::zeroed() }
            },
            sigusr1: sigaction {
                sa_sigaction: handle_sigusr1 as usize,
                sa_flags: SA_RESTART,
                sa_restorer: None,
                ..unsafe { std::mem::zeroed() }

            },
            sigkill: sigaction {
                sa_sigaction: handle_sigint_term as usize,
                sa_flags: SA_RESTART,
                sa_restorer: None,
                ..unsafe { std::mem::zeroed() }
            },
            hosts_context: program_ctx,
            hosts_len,
            colorize
        }
    }

    pub fn register_signals(&mut self) {
        
        self.set_sigint();
        self.set_sigusr1();
        self.set_sigterm();
    }

    pub fn unregister_signals() {
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            sigemptyset(&mut set);
            for &signal in [SIGINT, SIGUSR1, SIGTERM].iter() {
                libc::sigaddset(&mut set, signal);
            }
            sigprocmask(SIG_BLOCK, &set, ptr::null_mut());
            
        }
    }

    fn set_sigint(&mut self) {
        unsafe {
            
            sigemptyset(&mut self.sigint.sa_mask);
            if sigaction(SIGINT, &self.sigint, ptr::null_mut()) != 0 {
                eprintln!("register SIGINT");
                std::process::exit(3);
            }
        }
    }

    fn set_sigusr1(&mut self) {

        unsafe {
            sigemptyset(&mut self.sigusr1.sa_mask);
            PROGRAM_CONTEXT = self.hosts_context;
            HOSTS_LEN = self.hosts_len;
            COLORIZE = self.colorize;
            if sigaction(SIGUSR1, &self.sigusr1, ptr::null_mut()) != 0 {
                eprintln!("register SIGUSR1");
                std::process::exit(3);   
            }

        }
    }

    fn set_sigterm(&mut self) {
        unsafe {
            sigemptyset(&mut self.sigkill.sa_mask);
            if sigaction(SIGTERM, &self.sigkill, ptr::null_mut()) != 0 {
                eprintln!("register SIGTERM");
                std::process::exit(3);
            }

        }
    }

}


extern "C" fn handle_sigint_term(_signum: i32) {
    std::process::exit(4);
}


extern "C" fn handle_sigusr1(_signum: i32) {
    unsafe {
        if !PROGRAM_CONTEXT.is_null() {
            print_status();
        }
    }

}


extern "C" fn print_status() {
	let mut cp_ready = 0;
	let mut cp_running = 0;
	let mut cp_done = 0;
    
    
    unsafe {assert_eq!(HOSTS_LEN, (*PROGRAM_CONTEXT).len())};
    let magenta = unsafe {if COLORIZE {Color::Magenta} else {Color::White}};
    
    let hosts = unsafe { &*PROGRAM_CONTEXT };
    
    for host in hosts.iter() {
        
        match host.borrow().cp_status() {
            CpState::Ready => cp_ready += 1,
            CpState::Running => cp_running += 1,
            CpState::Done => cp_done += 1
        }
    } 
    
    
    println!("status: {} running {}, finished {}, remaining ({} total)", 
            cp_ready.to_string().as_str().colorize(&magenta), 
            cp_running.to_string().as_str().colorize(&magenta), 
            cp_done.to_string().as_str().colorize(&magenta),
            hosts.len().to_string().as_str().colorize(&magenta));
    
    if cp_running > 0 {
        println!("running processes:");

        for host in hosts.iter() {
            if let CpState::Running = host.borrow().cp_status() {
                println!("--> pid {} {}", host.borrow().cp_pid().to_string().as_str().colorize(&magenta), 
                    host.borrow().hostname().as_str().colorize(&magenta));
            }
        
        }

    }
}

