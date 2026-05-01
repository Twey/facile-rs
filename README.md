<!-- cargo-rdme start -->


# `facile`: build traits from their implementations

## Why?

In writing production Rust it's common to have a type that bundles a number of
different types, each of which expects some set of trait bounds in order to
‘function’ (have basic functionality implemented on it). Naïvely, this results
in propagating a large number of potentially quite verbose trait bounds
arbitrarily far up the program:

```rust
struct Foo<P, F> {
    pointer: P,
    future: F,
}

impl<P: AsRef<str>, F: Future<Output = ()> + Clone> Foo<P, F> {
    async fn run(&self) {
        println!("running future {}", self.pointer.as_ref());
        self.future.clone().await
    }
}

// These bounds must be propagated all the way up to the constructon of the
// `Foo`, where they are made concrete, repeated at every point. If the `impl`
// on `Foo` change constraints, all of these sites must change, even though
// they're just passing the constraints through.
async fn use_foo<P: AsRef<str>, F: Future<Output = ()> + Clone>(foo: &Foo<P, F>) {
    foo.run().await
}
```

You can fix this by replacing the type with a trait that bundles together the
constraints:

```rust
struct FooImpl<P, F> {
    pointer: P,
    future: F,
}

trait Foo {
    async fn run(&self);
}

impl<P: AsRef<str>, F: Future<Output = ()> + Clone> Foo for FooImpl<P, F> {
    async fn run(&self) {
        println!("running future {}", self.pointer.as_ref());
        self.future.clone().await
    }
}

// `impl Trait` syntax can often be used to make the resulting code even simpler.
async fn use_foo(foo: &impl Foo) {
    foo.run().await
}
```

but this can be annoying, especially for larger APIs, as it requires duplicating
the signatures of the entire façade.

Instead, this crate provides the `facade` attribute: write the implementation of
the façade as if you were implementing a non-existent trait, and the trait
itself will be auto-generated for you, bundling together the constraints and
implementation.

```rust

#[facile::facade]
impl<P: AsRef<str>, F: Future<Output = ()> + Clone> Foo for FooImpl<P, F> {
    async fn run(&self) {
        println!("running future {}", self.pointer.as_ref());
        self.future.clone().await
    }
}

async fn use_foo(foo: &impl Foo) {
    foo.run().await
}
```

<!-- cargo-rdme end -->
