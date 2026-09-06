# Detailed Design Record: DDR-0020
## Title: `martensite-macros` Declarative UI `widget!` Macro Design

### 1. Architectural Role & Invariants
`martensite-macros` provides a Rust-native procedural macro (`widget!`) to declare hierarchical UI without Virtual DOM or foreign DSLs.
* **Invariant 1.1**: The macro outputs 100% stable, idiomatic Rust. It does not reinvent borrow checking or control flow.
* **Invariant 1.2**: Lexical hygiene is maintained. User-provided signals or closures capture environment identically to vanilla Rust blocks.
* **Invariant 1.3**: Compile errors must map exactly to the user's source code span. No obfuscated macro-internal errors.

### 2. Token Tree Structure & Syntax
```rust
// User code:
widget! {
    VStack [ spacing: 10.0 ] {
        Text { content: "Hello, World!" }
        Button [ on_click: move || println!("Clicked") ] {
            Text { content: "Submit" }
        }
    }
}
```

### 3. Generated Code Translation
The macro parses the tree and expands to builder pattern API calls targeting the arena:
```rust
// Expanded equivalent:
{
    let mut _vstack = VStack::new().spacing(10.0);
    
    let mut _text = Text::new().content("Hello, World!");
    _vstack.add_child(_text.build());
    
    let mut _btn = Button::new().on_click(move || println!("Clicked"));
    let mut _btn_text = Text::new().content("Submit");
    _btn.add_child(_btn_text.build());
    
    _vstack.add_child(_btn.build());
    
    _vstack.build()
}
```

### 4. Parser Architecture (`syn` & `quote`)
- **Nodes**: Parse as `Ident`.
- **Properties**: Enclosed in `[` `]` or inline `{}`. Parse as `syn::Meta` or custom `Ident: Expr` pairs.
- **Children**: Nested `{}` blocks recursively parsed.

### 5. Error Reporting
If a property does not exist on a widget builder, the standard Rust compiler will natively complain: `no method named \`foo\` found for struct \`ButtonBuilder\``. The macro does not attempt to type-check; it only manipulates AST syntax tokens, delegating semantics to `rustc`.

### 6. Performance Invariants
- Proc-macro expansion time: < 50ms per average file.
- Generated code incurs zero runtime overhead compared to manual builder-pattern construction.
