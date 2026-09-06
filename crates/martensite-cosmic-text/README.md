# martensite-cosmic-text

This is a Martensite-project fork of [`cosmic-text`](https://github.com/pop-os/cosmic-text).

It exists because upstream `cosmic-text` 0.19 pins `fontdb` to `^0.23`, which
transitively depends on the unmaintained `ttf-parser` crate
([RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192)).
This fork updates `fontdb` to `0.24`, removing that dependency while preserving
the same public API.

## Changes from upstream

- `Cargo.toml`: `fontdb` version `0.23` -> `0.24`

The library name remains `cosmic_text`, so code using `use cosmic_text::...`
continues to compile unchanged when depending on this package.

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.
