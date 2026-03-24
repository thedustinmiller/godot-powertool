extends Node
## GameManager - Central game state manager autoload.
##
## This singleton manages global game state and provides access to the
## Rust GDExtension. Access it from anywhere via: GameManager
##
## Example:
##   var version = GameManager.get_extension_version()
##   GameManager.game_started.emit()

# =============================================================================
# Signals
# =============================================================================

## Emitted when a new game is started.
signal game_started

## Emitted when the game state changes.
signal game_state_changed

## Emitted when the game is paused or unpaused.
signal pause_changed(is_paused: bool)

# =============================================================================
# Properties
# =============================================================================

## Reference to the Rust extension instance (null when extension is not enabled).
var _extension: RefCounted = null

## Whether the extension is available.
var extension_available: bool = false

# =============================================================================
# Lifecycle
# =============================================================================

func _ready() -> void:
	_init_extension()
	print("GameManager ready. Extension available: ", extension_available)


func _init_extension() -> void:
	# Try to instantiate the Rust extension class dynamically
	if ClassDB.class_exists(&"Example"):
		_extension = ClassDB.instantiate(&"Example")
		extension_available = true
		print("Rust extension loaded. Version: ", _extension.get_version())
	else:
		push_warning("Rust extension not available. Build it with: cargo xtask build")
		extension_available = false

# =============================================================================
# Extension Access
# =============================================================================

## Get the Rust extension instance.
## Returns null if extension is not available.
func get_extension() -> RefCounted:
	return _extension


## Get the extension version string.
func get_extension_version() -> String:
	if _extension:
		return _extension.get_version()
	return "N/A"


## Check if the extension is loaded.
func has_extension() -> bool:
	return extension_available

# =============================================================================
# Game State
# =============================================================================

## Start a new game.
func start_game() -> void:
	print("Starting new game...")
	game_started.emit()
	game_state_changed.emit()


## Toggle pause state.
func toggle_pause() -> void:
	get_tree().paused = not get_tree().paused
	pause_changed.emit(get_tree().paused)
