fakeyou
=======

[![crates.io](https://img.shields.io/crates/v/fakeyou)](https://crates.io/crates/fakeyou)
[![fakeyou documentation](https://docs.rs/fakeyou/badge.svg)](https://docs.rs/fakeyou)

An easy, synchronous Rust library to access FakeYou's AI TTS services<br/>

***This library is a personal project and has no association with storyteller.ai***

# Usage
The first step in using this API is to authenticate. <br/>
In these examples, we are using a model token which is already known to us.<br/>
**These will take some time to finish, due to the API's queue**
```
use fakeyou;

fn main() {
    let fake_you = fakeyou::authenticate("user_name", "password").unwrap();
    fake_you.generate_file_from_token("Hello!", "TM:mc2kebvfwr1p", "hello.wav").unwrap();
}

```

You may also stream the resulting audio directly to an audio playback library, such as `rodio`:
```
use std::io::Cursor;
use rodio::{Decoder, OutputStream, source::Source, Sink};
use fakeyou;

fn main() {
    // rodio setup
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();

    // actual API use
    let fake_you = fakeyou::authenticate("user_name", "password").unwrap();
    let bytes = fake_you.generate_bytes_from_token("Hello!", "TM:mc2kebvfwr1p").unwrap();

    // play resulting audio
    let cursor = Cursor::new(bytes);
    let decoder = Decoder::new(cursor).unwrap();
    sink.append(decoder);
    sink.sleep_until_end();
}
```

### License
CC0 1.0 Universal