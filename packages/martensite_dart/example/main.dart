import 'package:martensite/martensite.dart';

/// Demonstrates initializing the Martensite engine and creating reactive signals.
void main() {
  // Initialize the singleton Martensite coordinator
  final engine = MartensiteEngine.initialize();

  // Create a reactive integer signal
  final counter = engine.createSignal<int>(0);
  print('Initial counter value: ${counter.value}');

  // Mutate the signal value
  counter.update((val) => val + 1);
  print('Updated counter value: ${counter.value}');

  // Inspect widget handle semantics
  const handle = WidgetHandle(slotIndex: 1, generation: 1);
  print('Created handle: $handle with 64-bit ID: ${handle.toInt64()}');
}
