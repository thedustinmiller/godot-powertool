extends GutTest
## Unit tests for the Rust extension Example class.
##
## Uncomment when the extension is enabled in the workspace.
## To enable: add "extension" to members in root Cargo.toml,
## then run: cargo xtask build
##
## Run with: cargo xtask test

#
# All tests below are commented out because the extension is disabled by default.
# Uncomment them after enabling and building the Rust GDExtension.
#

#func test_extension_class_exists() -> void:
#	if not ClassDB.class_exists(&"Example"):
#		pending("Extension not loaded")
#		return
#	assert_true(true, "Example class is registered via GDExtension")
#
#
#func test_extension_node_class_exists() -> void:
#	if not ClassDB.class_exists(&"ExampleNode"):
#		pending("Extension not loaded")
#		return
#	assert_true(true, "ExampleNode class is registered via GDExtension")
#
#
#var _example: RefCounted = null
#
#
#func before_each() -> void:
#	if ClassDB.class_exists(&"Example"):
#		_example = ClassDB.instantiate(&"Example")
#
#
#func after_each() -> void:
#	_example = null
#
#
#func test_example_instantiation() -> void:
#	if not ClassDB.class_exists(&"Example"):
#		pending("Extension not loaded")
#		return
#	assert_not_null(_example, "Should create Example instance")
#
#
#func test_example_default_greeting() -> void:
#	if not ClassDB.class_exists(&"Example"):
#		pending("Extension not loaded")
#		return
#	var greeting = _example.get_greeting()
#	assert_eq(greeting, "Hello from Rust!", "Default greeting should be set")
#
#
#func test_example_set_message() -> void:
#	if not ClassDB.class_exists(&"Example"):
#		pending("Extension not loaded")
#		return
#	_example.set_message("Custom message")
#	var greeting = _example.get_greeting()
#	assert_eq(greeting, "Custom message", "Message should be updated")
#
#
#func test_example_add() -> void:
#	if not ClassDB.class_exists(&"Example"):
#		pending("Extension not loaded")
#		return
#	assert_eq(_example.add(2, 3), 5, "2 + 3 should equal 5")
#	assert_eq(_example.add(-1, 1), 0, "-1 + 1 should equal 0")
#	assert_eq(_example.add(0, 0), 0, "0 + 0 should equal 0")
#
#
#func test_example_version() -> void:
#	if not ClassDB.class_exists(&"Example"):
#		pending("Extension not loaded")
#		return
#	var version = ClassDB.instantiate(&"Example").get_version()
#	assert_not_null(version, "Version should not be null")
#	assert_true(version.length() > 0, "Version should not be empty")
#
#
#func test_example_node_instantiation() -> void:
#	if not ClassDB.class_exists(&"ExampleNode"):
#		pending("Extension not loaded")
#		return
#	var node = ClassDB.instantiate(&"ExampleNode")
#	assert_not_null(node, "Should create ExampleNode instance")
#	assert_true(node is Node, "ExampleNode should extend Node")
#	node.queue_free()
#
#
#func test_example_node_default_values() -> void:
#	if not ClassDB.class_exists(&"ExampleNode"):
#		pending("Extension not loaded")
#		return
#	var node = ClassDB.instantiate(&"ExampleNode")
#	assert_eq(node.get_ticks(), 0, "Initial tick count should be 0")
#	assert_eq(node.speed, 100.0, "Default speed should be 100.0")
#	node.queue_free()
#
#
#func test_example_node_reset() -> void:
#	if not ClassDB.class_exists(&"ExampleNode"):
#		pending("Extension not loaded")
#		return
#	var node = ClassDB.instantiate(&"ExampleNode")
#	node.tick_count = 100
#	assert_eq(node.get_ticks(), 100, "Tick count should be set")
#	node.reset()
#	assert_eq(node.get_ticks(), 0, "Tick count should be reset to 0")
#	node.queue_free()
