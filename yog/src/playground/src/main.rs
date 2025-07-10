use mockall::automock;

struct NonClone;

#[automock]
trait Foo {
    fn foo(&self) -> Result<NonClone, Box<dyn std::error::Error>>;
}

fn main() {
    let mut mock = MockFoo::new();

    let mut seq = mockall::Sequence::new();

    mock.expect_foo()
        .times(1)
        .in_sequence(&mut seq)
        .return_once(move || Ok(NonClone {}));

    mock.expect_foo()
        .times(1)
        .in_sequence(&mut seq)
        .return_once(move || Ok(NonClone {}));

    let _ = mock.foo();
    let _ = mock.foo();
}
