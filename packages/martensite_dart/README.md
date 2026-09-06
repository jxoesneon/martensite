# Martensite Dart

[![pub package](https://img.shields.io/pub/v/martensite.svg)](https://pub.dev/packages/martensite)
[![package publisher](https://img.shields.io/pub/publisher/martensite.svg)](https://pub.dev/packages/martensite/publisher)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT)

Official Dart and Flutter bindings for **Martensite**, the retained-mode, GPU-accelerated graphical user interface framework written in Rust.

## Features

* **Sub-Millisecond Native Bridge**: Direct zero-copy FFI integration between the Dart VM and Martensite native memory arenas.
* **Fine-Grained Reactive Signals**: Synchronized push-pull signal graph bridging Dart streams and Rust signals.
* **100% Platform Parity**: Native hardware acceleration across Windows, macOS, and Linux desktop environments.

## Getting Started

Add `martensite` to your `pubspec.yaml`:

```yaml
dependencies:
  martensite: ^0.1.0
```

## Usage

```dart
import 'package:martensite/martensite.dart';

void main() {
  final engine = MartensiteEngine.initialize();
  final signal = engine.createSignal<int>(42);
  print('Active Signal Value: ${signal.value}');
}
```

## Additional Information

For complete architecture documentation, visit the [Martensite Documentation Portal](https://martensite.dev) or the [GitHub Repository](https://github.com/jxoesneon/martensite).
