<h1 align="center">
<div>
<img src="./media/sshp4ru_logo.png" alt="logo" style="width:400px;margin-bottom:0.1vh">
</div>
<strong>sshp4ru - Parallel SSH Executor in Rust</strong>
</h1>

<h4 align="center">
    <img src="https://img.shields.io/badge/License-MIT-%2300599C.svg" alt="MIT" style="height: 20px;">
    <img src="https://img.shields.io/badge/Rust-1.82-%23006B3F.svg?logo=rust&logoColor=white" alt="rust" style="height: 20px;">
</h4>

### *sshp4ru  is a high-performance, parallel SSH executor, rewritten in **Rust**, based on the C-based [`sshp`][sshp] project by [Dave Eddy][dave-eddy] and [Rany]. This tool is designed to execute SSH commands concurrently across multiple hosts, managing the associated SSH connections efficiently and coalescing the output in a systematic manner.*


# 🚩 Table of Contents
* [About](#about)
* [Installation](#installation)
* [Functionality and Interface](#functionality-and-interface)
* [Testing and Style](#testing-and-style)
* [Future Work](#future-work)
* [Contact](#contact)
* [License](#license)


# About

The original [`node-sshp`][sshp-node] was developed in **Node.js** and was later rereleased in **C**. This adaptation was based off the **C** version, which offered a straightforward solution for managing multiple SSH processes in parallel. The project was well-regarded for its ability to handle a large number of concurrent SSH connections while efficiently managing their output. The **sshp4ru** project re-implements this functionality in **Rust**, a systems programming language known for its memory safety, concurrency and performance guarantees. This rewrite ensures that the tool remains robust and resilient to memory-related issues.

# Installation

Before using `sshp4ru`, make sure you have **Rust** >= **2021** installed on your system.
1. Clone the repository and navigate to the project's directory:

   ```bash
   git clone https://github.com/yourusername/sshp4ru.git
   cd sshp4ru
   ```
2. Install the CLI binary:
    ```bash
    cargo install --path . --release
    ```
> [!NOTE]  
> The compiled binary will be located in the  ~/.cargo/bin directory (which should be in PATH). 


3. **Alternatively**, compile the CLI binary and copy it manually:
    ```bash
    cargo build --release
    cp ./target/release/sshp4ru /usr/local/bin
    ```
  
4. Verify the installation by checking the version:
    ```bash
    sshp4ru --version
    ```

# Functionality and Interface

- The functionality of `sshp4ru` is identical to the [C-based implementation][sshp] of `sshp`. The core features and behavior have been preserved.

- The [command-line interface (CLI)][usage] remains unchanged, meaning you can use `sshp4ru` in the same way you would use `sshp`. The only difference is the name of the executable (`sshp4ru`) when running commands. 

- The handling of exit codes follows the same conventions as the `sshp` implementation, ensuring compatibility. For detailed information on how exit codes are used, you can refer to the documentation of [`sshp`][exit-codes].

- To check the functionality of `sshp4ru`, you can refer to the [examples] from the [C-based version][sshp]. The interface, arguments and expected results will be the same.

### Example:

If the C-based implementation command was:
```bash
sshp -f hosts.txt -m 3 uname -v
```
The equivalent command with sshp4ru would be:
```bash
sshp4ru -f hosts.txt -m 3 uname -v
```

# Testing and Style

The test suite included in this version of the project was originally part of the [C-based implementation][sshp] and has since been adapted to the Rust version.

> [!NOTE]  
> In order to run the tests, you should use cargo build instead of cargo build --release. The compiled binary will be located in the ./target/debug/ directory when using the standard build configuration.

# Future Work
In the pursuit of self-improvement, any new suggestions, solutions and potential bug fixes are welcome. Just **open an issue** or **submit a pull request**.

The following features and enhancements are potential candidates for future development:

- Implementing ``kqueue`` for event monitoring to enhance event handling on systems that support **kqueue**.
- Publish the project on **crates.io**.
- Adding a **password login timer** to enforce a time limit for password-based logins.


# Contact

- Sofotasios Argiris | <a href="mailto:a.sofotasios@ceid.upatras.gr">a.sofotasios@ceid.upatras.gr</a>
- Metaxakis Dimitris | <a href="mailto:d.metaxakis@ceid.upatras.gr">d.metaxakis@ceid.upatras.gr</a>

# License

Distributed under the [MIT][license-link] License. See `LICENSE.md` for more details.

<!-- MARKDOWN LINKS & IMAGES -->
[dave-eddy]: https://github.com/bahamas10
[sshp]: https://github.com/bahamas10/sshp
[sshp-node]: https://github.com/bahamas10/node-sshp
[license-link]: https://github.com/DmMeta/ChordSeek/blob/main/LICENSE
[exit-codes]: https://github.com/bahamas10/sshp?tab=readme-ov-file#exit-codes
[examples]: https://github.com/bahamas10/sshp?tab=readme-ov-file#examples
[usage]: https://github.com/bahamas10/sshp?tab=readme-ov-file#usage
[rany]: https://github.com/rany2