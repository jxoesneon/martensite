import 'package:martensite/martensite.dart';
import 'package:test/test.dart';

void main() {
  group('Martensite Dart Bindings', () {
    test('version string is defined', () {
      expect(martensiteVersion, equals('0.0.2'));
    });

    test('widget handle encoding', () {
      const handle = WidgetHandle(slotIndex: 42, generation: 7);
      expect(handle.slotIndex, equals(42));
      expect(handle.generation, equals(7));
    });

    test('signal reactivity', () {
      final engine = MartensiteEngine.initialize();
      final signal = engine.createSignal<int>(10);
      expect(signal.value, equals(10));

      signal.value = 20;
      expect(signal.value, equals(20));

      signal.update((v) => v * 2);
      expect(signal.value, equals(40));
    });
  });
}
