extends GutTest
## Unit tests for the GameManager autoload.
##
## These tests verify the GameManager singleton functionality.
## Run with: cargo xtask test


# =============================================================================
# Test: GameManager Existence
# =============================================================================

func test_game_manager_exists() -> void:
	# GameManager should be available as an autoload
	var gm = get_node_or_null("/root/GameManager")
	assert_not_null(gm, "GameManager autoload should exist")


func test_game_manager_is_correct_type() -> void:
	var gm = get_node_or_null("/root/GameManager")
	if gm:
		assert_true(gm is Node, "GameManager should be a Node")


# =============================================================================
# Test: Extension Access
# =============================================================================

func test_has_extension() -> void:
	# This test will pass regardless of whether extension is loaded
	# It just verifies the method exists and returns a boolean
	var result = GameManager.has_extension()
	assert_true(result is bool, "has_extension should return boolean")


func test_get_extension_version() -> void:
	var version = GameManager.get_extension_version()
	assert_not_null(version, "Version should not be null")

	if GameManager.has_extension():
		assert_true(version.length() > 0, "Version should not be empty when extension loaded")
	else:
		assert_eq(version, "N/A", "Version should be N/A when extension not loaded")


func test_get_extension() -> void:
	var ext = GameManager.get_extension()

	if GameManager.has_extension():
		assert_not_null(ext, "Extension should not be null when available")
	else:
		assert_null(ext, "Extension should be null when not available")


# =============================================================================
# Test: Signals
# =============================================================================

func test_game_started_signal() -> void:
	watch_signals(GameManager)

	GameManager.start_game()

	assert_signal_emitted(GameManager, "game_started", "game_started signal should emit")
	assert_signal_emitted(GameManager, "game_state_changed", "game_state_changed signal should emit")


func test_pause_changed_signal() -> void:
	watch_signals(GameManager)

	# Ensure we start unpaused
	get_tree().paused = false

	GameManager.toggle_pause()
	assert_signal_emitted(GameManager, "pause_changed", "pause_changed signal should emit")

	# Clean up - unpause
	if get_tree().paused:
		GameManager.toggle_pause()
