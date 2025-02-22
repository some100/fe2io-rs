# fe2io-rs

Lighterweight alternative to fe2.io written in Rust

## Usage

`fe2io-rs username [volume] [url]`

- Username is required. It is the player's username.
- Volume is optional, and is the volume of the audio on death. Default is 0.5
- URL is optional, and is the server the application connects to. Default is `ws://client.fe2.io:8081`

## Compilation

fe2io-rs is written in Rust, so the Cargo toolchain is required for compilation.

```
git clone https://github.com/some100/fe2io-rs.git
cargo build -r
```

## Acknowledgements

Thanks @richardios275 for the fe2io-python project, which was overall an inspiration for this being my first real Rust project

Thanks @Crazyblox for making FE2 and creating the Flood Escape genre as a whole (i think tria is a better game though)
