extends GutTest
## Integration tests for the main scene.
##
## These tests verify the main scene loads and functions correctly.
## Run with: cargo xtask test


const MAIN_SCENE = preload("res://scenes/main.tscn")


# =============================================================================
# Test: Scene Loading
# =============================================================================

func test_main_scene_loads() -> void:
	var scene = MAIN_SCENE.instantiate()
	assert_not_null(scene, "Main scene should instantiate")
	scene.queue_free()


func test_main_scene_has_required_nodes() -> void:
	var scene = MAIN_SCENE.instantiate()
	add_child(scene)

	# Check for expected UI elements
	var status_label = scene.get_node_or_null("VBoxContainer/StatusLabel")
	var version_label = scene.get_node_or_null("VBoxContainer/VersionLabel")
	var result_label = scene.get_node_or_null("VBoxContainer/ResultLabel")

	assert_not_null(status_label, "StatusLabel should exist")
	assert_not_null(version_label, "VersionLabel should exist")
	assert_not_null(result_label, "ResultLabel should exist")

	scene.queue_free()


func test_main_scene_buttons_exist() -> void:
	var scene = MAIN_SCENE.instantiate()
	add_child(scene)

	var start_button = scene.get_node_or_null("VBoxContainer/ButtonContainer/StartButton")
	var quit_button = scene.get_node_or_null("VBoxContainer/ButtonContainer/QuitButton")

	assert_not_null(start_button, "StartButton should exist")
	assert_not_null(quit_button, "QuitButton should exist")

	scene.queue_free()


# =============================================================================
# Test: UI State
# =============================================================================

func test_status_updates_based_on_extension() -> void:
	var scene = MAIN_SCENE.instantiate()
	add_child(scene)

	# Give time for _ready to run
	await get_tree().process_frame

	var status_label: Label = scene.get_node("VBoxContainer/StatusLabel")

	if GameManager.has_extension():
		assert_true(
			"Loaded" in status_label.text,
			"Status should indicate extension is loaded"
		)
	else:
		assert_true(
			"Ready" in status_label.text,
			"Status should indicate ready state"
		)

	scene.queue_free()
