extends GutTest
## Unit tests for the optional sample_extension Rust GDExtension.
##
## When the extension isn't built / enabled, tests pend with a skip note
## rather than fail — the test suite is the binary-or-slow CI gate from
## native-split.md.
##
## To enable: uncomment "addons/sample_extension/rust" in the root Cargo.toml
## members list, then run `cargo xtask build`. Run with: cargo xtask test


func _skip_if_extension_missing() -> bool:
	if not ClassDB.class_exists(&"SampleGreeter"):
		pending("sample_extension not loaded — class SampleGreeter is not registered")
		return true
	return false


func test_sample_greeter_class_exists() -> void:
	if _skip_if_extension_missing():
		return
	assert_true(
		ClassDB.class_exists(&"SampleGreeter"),
		"SampleGreeter class is registered via GDExtension"
	)


func test_sample_greeter_instantiation() -> void:
	if _skip_if_extension_missing():
		return
	var greeter: Object = ClassDB.instantiate(&"SampleGreeter")
	assert_not_null(greeter, "Should create SampleGreeter instance")


func test_sample_greeter_greet() -> void:
	if _skip_if_extension_missing():
		return
	var greeter: Object = ClassDB.instantiate(&"SampleGreeter")
	assert_eq(greeter.greet("World"), "Hello, World!", "greet round-trips a name into a greeting")
	assert_eq(greeter.greet(""), "Hello, !", "greet handles empty input")


func test_sample_greeter_fibonacci() -> void:
	if _skip_if_extension_missing():
		return
	var greeter: Object = ClassDB.instantiate(&"SampleGreeter")
	assert_eq(greeter.fibonacci(0), 0, "fib(0) = 0")
	assert_eq(greeter.fibonacci(1), 1, "fib(1) = 1")
	assert_eq(greeter.fibonacci(10), 55, "fib(10) = 55")
	assert_eq(greeter.fibonacci(40), 102334155, "fib(40) = 102334155")
