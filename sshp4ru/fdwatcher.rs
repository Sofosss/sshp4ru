use crate::utils::{Color, Colorize};
use crate::RuntimeError;
use crate::{Host, ProgMode};
use epoll;
use nix::unistd::close;
use std::cell::RefCell;
use std::io::{self, Write};
use std::os::fd::RawFd;
use std::rc::Rc;

#[cfg(feature = "USE_KQUEUE")]
use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent, Kqueue};

#[derive(Debug, Clone)]
pub enum PipeType {
    StdOut = 0,
    StdErr,
    StdIO,
}

#[derive(Debug)]
pub struct FdEvent {
    host: Rc<RefCell<Host>>,
    fd: i32,
    buffer: String,
    offset: usize,
    event_type: PipeType,
}

impl FdEvent {
    pub fn new(host: Rc<RefCell<Host>>, event_type: PipeType) -> Self {
        let ev_type = event_type.clone();
        let mut fdev = FdEvent {
            host: host.clone(),
            buffer: String::new(),
            offset: 0,
            fd: 0,
            event_type: event_type,
        };
        //different type of buffering will be implemented on subsequent layers.
        match ev_type {
            PipeType::StdOut => fdev.fd = host.borrow().cp.stdout_fd,
            PipeType::StdErr => fdev.fd = host.borrow().cp.stderr_fd,
            PipeType::StdIO => fdev.fd = host.borrow().cp.stdio_fd,
        }

        assert!(fdev.fd > 0);
        fdev
    }

    pub fn read_active_fd(
        &mut self, watcher: &Fdwatcher, last_host: &mut Option<String>, newline_print: &mut bool,
        config_params: impl FnOnce() -> (bool, ProgMode, u16, u16, bool, bool),
    ) -> Result<bool, RuntimeError> {
        let mut buffer = [0u8; 8192];
        let (silent, mode, max_line_length, max_output_length, anonymous_opt, colorize) =
            config_params();

        let mut fd: RawFd = match self.event_type {
            PipeType::StdIO => self.host.borrow_mut().cp.stdio_fd,
            PipeType::StdOut => self.host.borrow_mut().cp.stdout_fd,
            PipeType::StdErr => self.host.borrow_mut().cp.stderr_fd,
        };

        loop {
            match nix::unistd::read(fd, &mut buffer) {
                Ok(0) => {
                    watcher.remove(fd)?;
                    if let Err(e) = close(fd) {
                        return Err(RuntimeError::CloseFdError(e));
                    }
                    fd = -2;

                    match self.event_type {
                        PipeType::StdIO => self.host.borrow_mut().cp.stdio_fd = fd,
                        PipeType::StdOut => self.host.borrow_mut().cp.stdout_fd = fd,
                        PipeType::StdErr => self.host.borrow_mut().cp.stderr_fd = fd,
                    }

                    match mode {
                        ProgMode::Join => self.output_join_buf(max_output_length),
                        ProgMode::Group => (),
                        ProgMode::Line => self.output_line_buf(anonymous_opt, colorize),
                    }

                    return Ok(true);
                }

                Ok(bytes_read) => {
                    if silent {
                        continue;
                    }

                    match mode {
                        ProgMode::Join => self.process_join_buf(
                            &buffer[..bytes_read],
                            max_line_length,
                            max_output_length,
                        ),
                        ProgMode::Group => {
                            if let Err(_) = self.process_group_buf(
                                &buffer[..bytes_read],
                                &last_host,
                                anonymous_opt,
                                newline_print,
                                colorize,
                            ) {
                                return Err(RuntimeError::WriteStreamError);
                            }
                            *last_host = Some(self.host.borrow().name.clone());
                        }
                        ProgMode::Line => self.process_line_buf(
                            &buffer[..bytes_read],
                            max_line_length,
                            anonymous_opt,
                            colorize,
                        ),
                    }
                }

                Err(e) => {
                    if e == nix::errno::Errno::EWOULDBLOCK {
                        return Ok(false);
                    }

                    return Err(RuntimeError::ReadFdError(e));
                }
            }
        } //loop
    }

    // pub fn hostname(&self) -> String {
    //     self.host.borrow().name.clone()
    // }

    pub fn get_host(&self) -> Rc<RefCell<Host>> {
        self.host.clone()
    }

    fn output_join_buf(&mut self, max_output_length: u16) {
        if self.offset <= max_output_length as usize {
            if !self.buffer.ends_with("\n") {
                self.buffer.push('\n');
                self.offset += 1;
            }
        }
        // explicitly move buffer to output_buffer of host to avoid unnecessary copying
        self.host.borrow_mut().cp.output_buffer = std::mem::take(&mut self.buffer);
    }

    fn process_join_buf(&mut self, buffer: &[u8], max_line_length: u16, max_output_length: u16) {
        for ch in buffer.iter() {
            if self.offset < max_output_length as usize {
                let ch_ascii = if ch.is_ascii() { *ch as char } else { '?' };
                self.buffer.push(ch_ascii);
                self.offset += 1;
            } else if self.offset == max_line_length as usize {
                //\n or something else?
                self.buffer.push('\n');
                self.offset += 1;
            } else {
                break;
            }
        }
    }

    fn process_group_buf(
        &mut self, buffer: &[u8], last_host: &Option<String>, anonymous_opt: bool,
        newline_print: &mut bool, colorize: bool,
    ) -> io::Result<()> {
        let cyan = if colorize { Color::Cyan } else { Color::Empty };
        //maybe somewhat ugly but gets rid of potential unsafe mutation on static last_host and newline_print
        if let Some(last_host) = last_host {
            if last_host.as_str() != self.host.borrow().name.as_str() {
                if !*newline_print {
                    println!();
                }
                if !anonymous_opt {
                    println!("[{}]", self.host.borrow().name.as_str().colorize(&cyan));
                }
            }
        } else {
            if !*newline_print {
                println!();
            }
            if !anonymous_opt {
                println!("[{}]", self.host.borrow().name.as_str().colorize(&cyan));
            }
        }

        let color = if !colorize {
            Color::Empty.as_str()
        } else {
            match self.event_type {
                PipeType::StdOut => Color::Green.as_str(),
                PipeType::StdErr => Color::Red.as_str(),
                _ => Color::Reset.as_str(),
            }
        };
        let mut writer = io::BufWriter::new(io::stdout().lock());
        writer.flush()?;

        writer.write(color.as_bytes())?;
        writer.write(buffer)?;
        if colorize {
            writer.write(Color::Reset.as_str().as_bytes())?;
        }

        *newline_print = buffer[buffer.len() - 1] != b'\n';

        Ok(())
    }

    fn process_line_buf(
        &mut self, buffer: &[u8], max_line_length: u16, anonymous_opt: bool, colorize: bool,
    ) {
        // println!("{}", buffer.len());
        for ch in buffer.iter() {
            if self.offset < max_line_length as usize {
                let ch_ascii = if ch.is_ascii() { *ch as char } else { '?' };
                self.buffer.push(ch_ascii);
                self.offset += 1;
            } else if self.offset == max_line_length as usize {
                self.buffer.push('\n');
                self.offset += 1;
            }

            if *ch == b'\n' {
                assert!(self.offset > 0);
                assert!(self.offset < max_line_length as usize + 2);
                self.print_line_buffer(anonymous_opt, colorize);
                self.offset = 0;
                self.buffer.clear();
            }
        }
    }

    fn output_line_buf(&mut self, anonymous_opt: bool, colorize: bool) {
        if self.offset == 0 {
            return;
        }

        self.print_line_buffer(anonymous_opt, colorize);
        self.offset = 0;
    }

    fn print_line_buffer(&self, anonymous_option: bool, colorize: bool) {
        let (color, cyan) = if !colorize {
            (Color::Empty, Color::Empty)
        } else {
            (
                match self.event_type {
                    PipeType::StdOut => Color::Green,
                    PipeType::StdErr => Color::Red,
                    _ => Color::Reset,
                },
                Color::Cyan,
            )
        };

        if !anonymous_option {
            print!("[{}] ", self.host.borrow().name.as_str().colorize(&cyan));
        }

        if let Some(last_char) = self.buffer.chars().rev().next() {
            if last_char != '\n' {
                println!("{}", self.buffer.as_str().colorize(&color));
            } else {
                print!("{}", self.buffer.as_str().colorize(&color));
            }
        }
    }
}

#[derive(Debug)]
pub struct Fdwatcher {
    #[cfg(feature = "USE_KQUEUE")]
    kq: kqueue::Watcher,
    #[cfg(not(feature = "USE_KQUEUE"))]
    epoll: i32,
}

impl Fdwatcher {
    #[cfg(feature = "USE_KQUEUE")]
    pub fn new() -> Self {
        Self::new_kqueue()
    }

    #[cfg(not(feature = "USE_KQUEUE"))]
    pub fn new() -> io::Result<Self> {
        Ok(Self::new_epoll()?)
    }

    #[cfg(not(feature = "USE_KQUEUE"))]
    fn new_epoll() -> io::Result<Self> {
        let epoll_fd = epoll::create(true)?;
        Ok(Self { epoll: epoll_fd })
    }

    #[cfg(feature = "USE_KQUEUE")]
    fn new_kqueue() -> io::Result<Self> {
        let kq = Kqueue::new()?;

        Ok(Self { kq: kq })
    }

    #[cfg(feature = "USE_KQUEUE")]
    pub fn add(&mut self, monitor_fd: i32) -> io::Result<()> {
        //not implemented yet. Probably doesn't work as expected
        let event = KEvent::new(
            monitor_fd as u64,
            EventFilter::EVFILT_READ,
            EventFlag::EV_ADD,
            FilterFlag::empty(),
            0,
            0,
        );
        self.kq.kevent(&[event], &[], None)?;

        Ok(())
    }

    #[cfg(not(feature = "USE_KQUEUE"))]
    pub fn add(&self, monitor_fd: i32) -> io::Result<()> {
        let event = epoll::Event::new(epoll::Events::EPOLLIN, monitor_fd as u64);
        if let Err(e) = epoll::ctl(
            self.epoll,
            epoll::ControlOptions::EPOLL_CTL_ADD,
            monitor_fd,
            event,
        ) {
            Err(e)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "USE_KQUEUE")]
    pub fn wait(
        &self, completed_events: &mut [RawFd], num_events: usize, timeout: i32,
    ) -> io::Result<()> {
        //not implemented yet. Probably doesn't work as expected

        let mut num_completed_events: usize = 0;

        while num_completed_events < num_events {
            if let Some(event) = self.kq.poll_forever(None) {
                match event {
                    kqueue::Ident::Fd(fd) => {
                        completed_events[num_completed_events] = fd;
                    }
                    _ => return Err(io::Error::new(io::ErrorKind::Other, "Invalid event type")),
                }
            }
            num_completed_events += 1;
        }
        Ok(num_completed_events)
    }

    #[cfg(not(feature = "USE_KQUEUE"))]
    pub fn wait(
        &self, completed_events: &mut [RawFd], num_events: usize, timeout: i32,
    ) -> Result<usize, RuntimeError> {
        let mut epoll_events = vec![epoll::Event::new(epoll::Events::empty(), 0); num_events];
        // // epoll::wait, unlike epoll_wait() (libc) does not take a max events argument,
        // // it calculates it internally from the size of the given slice (here epoll_events)
        let num_completed_events = match epoll::wait(self.epoll, timeout, &mut epoll_events) {
            Ok(n) => n,
            Err(e) => return Err(RuntimeError::EpollWaitError(e)),
        };

        for (i, event) in epoll_events[0..num_completed_events].iter().enumerate() {
            completed_events[i] = event.data as i32;
        }

        Ok(num_completed_events)
    }

    #[cfg(feature = "USE_KQUEUE")]
    fn remove(&self, monitor_fd: i32) -> io::Result<()> {
        //not implemented yet. Probably doesn't work as expected
        self.kq
            .remove_fd(monitor_fd, kqueue::EventFilter::EVFILT_READ)?;
        Ok(())
    }

    #[cfg(not(feature = "USE_KQUEUE"))]
    fn remove(&self, monitor_fd: i32) -> Result<(), RuntimeError> {
        let event = epoll::Event::new(epoll::Events::EPOLLIN, monitor_fd as u64);
        if let Err(_) = epoll::ctl(
            self.epoll,
            epoll::ControlOptions::EPOLL_CTL_DEL,
            monitor_fd,
            event,
        ) {
            Err(RuntimeError::MonitorFdError("EPOLL_CTL_DEL".to_string()))
        } else {
            Ok(())
        }
    }
}
