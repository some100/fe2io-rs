# fe2io-rs

Lighterweight alternative to fe2.io written in Rust

## Usage

Run `fe2io-rs --help` and see what happens.

- Username is required. It is the player's username.
- Volume is optional, and is the volume of the audio on death. Default is 0.5
- URL is optional, and is the server the application connects to. Default is `ws://client.fe2.io:8081`

## Use case

Browsers are very heavy on resources. Games like FE2 are also very heavy on resources. Since fe2.io works using a web browser, normally when playing maps on FE2CM you'll be forced to open a browser in the background taking up resources needlessly to do one relatively simple thing. The issue is especially pronounced when doing much harder maps, where consistent framerates are a massive benefit. This is fixed by using a standalone app, similar to the https://github.com/richardios275/fe2io-python project. However, this project is much more lightweight due to forgoing the GUI and being totally independent in function, since it doesn't require ffmpeg for conversions (due to this, this project manages basically the same functionality in nearly 1/10 the diskspace.)

## Compilation

fe2io-rs is written in Rust, so the Cargo toolchain is required for compilation.

```
git clone https://github.com/some100/fe2io-rs.git
cargo build -r
```

## Acknowledgements

Thanks [@richardios275](https://github.com/richardios275) for the fe2io-python project, which was overall an inspiration for this being my first real Rust project

Thanks [@Crazyblox](https://github.com/Crazyblox) for making FE2 and creating the Flood Escape genre as a whole (i think tria is a better game though)
