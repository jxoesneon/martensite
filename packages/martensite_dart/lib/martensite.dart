/// Official Dart and Flutter client library for the Martensite GUI engine.
///
/// Provides zero-copy foreign function interface (FFI) bindings,
/// generational slotmap node management, and push-pull reactive signals.
library martensite;

import 'dart:ffi' as ffi;

/// The canonical semantic version string of the Martensite engine.
const String martensiteVersion = '0.0.1';

/// Represents a persistent 64-bit handle to a node inside the Martensite generational arena.
class WidgetHandle {
  /// The sparse slot index in the arena.
  final int slotIndex;

  /// The generational epoch counter guarding against use-after-free bugs.
  final int generation;

  /// Creates a new immutable [WidgetHandle].
  const WidgetHandle({
    required this.slotIndex,
    required this.generation,
  });

  /// Converts the handle into a single 64-bit integer representation.
  int toInt64() => (generation << 32) | (slotIndex & 0xFFFFFFFF);

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is WidgetHandle &&
          runtimeType == other.runtimeType &&
          slotIndex == other.slotIndex &&
          generation == other.generation;

  @override
  int get hashCode => slotIndex.hashCode ^ generation.hashCode;

  @override
  String toString() => 'WidgetHandle(slot: $slotIndex, gen: $generation)';
}

/// A reactive signal cell encapsulating a value of type [T].
class Signal<T> {
  /// Unique identifier of the signal inside the reactive Directed Acyclic Graph.
  final int id;

  T _value;

  /// Creates a new [Signal] with an initial [value].
  Signal(this.id, this._value);

  /// Reads the current value, automatically tracking dependencies when in a reactive context.
  T get value => _value;

  /// Updates the value and flags dependent nodes in the reactive graph.
  set value(T newValue) {
    if (_value != newValue) {
      _value = newValue;
    }
  }

  /// Mutates the current value using an update callback [fn].
  void update(T Function(T current) fn) {
    value = fn(_value);
  }

  @override
  String toString() => 'Signal#$id($value)';
}

/// The primary coordinator for Martensite native execution.
class MartensiteEngine {
  static MartensiteEngine? _instance;
  int _nextSignalId = 1;

  MartensiteEngine._();

  /// Initializes and retrieves the singleton [MartensiteEngine] instance.
  static MartensiteEngine initialize() {
    _instance ??= MartensiteEngine._();
    return _instance!;
  }

  /// Allocates a new reactive [Signal] containing the provided [initial] value.
  Signal<T> createSignal<T>(T initial) {
    final sig = Signal<T>(_nextSignalId++, initial);
    return sig;
  }
}
