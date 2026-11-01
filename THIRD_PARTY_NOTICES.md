# Third-party notices

Parts of `crates/sony-protocol` and the platform RFCOMM connection pattern in
`crates/transport` were adapted from
[`aecca/sound-connect`](https://github.com/aecca/sound-connect), used under the
MIT License:

> Copyright (c) 2026 aecca and contributors

The complete license text is available at
`crates/sony-protocol/LICENSE.sound-connect-MIT`.

AirPods AAP frame layouts, setting identifiers, initialization packets, and test vectors in
`crates/airpods-protocol` were reviewed against
[`Rockykln/podctl`](https://github.com/Rockykln/podctl), available under MIT OR Apache-2.0. This
project uses those protocol facts and a local implementation; it does not vendor the podctl
runtime. The MIT license text is preserved at
`crates/airpods-protocol/LICENSE.podctl-MIT`.
