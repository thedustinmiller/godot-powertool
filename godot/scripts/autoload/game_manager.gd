extends Node
## GameManager - Central game state manager autoload.
##
## This singleton manages global game state and provides access to the
## optional Rust GDExtension (sample_extension). Access it from anywhere via:
## GameManager
##
## Example:
##   if GameManager.has_extension():
##       print(GameManager.get_extension().greet("World"))
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
	# Probe for the sample_extension class. Mirrors the defensive pattern in
	# addons/sample_extension/plugin.gd — extension is fully optional.
	if ClassDB.class_exists(&"SampleGreeter"):
		_extension = ClassDB.instantiate(&"SampleGreeter")
		extension_available = true
		print("Rust extension loaded: SampleGreeter from sample_extension.")
	else:
		print("Rust extension (sample_extension) not loaded — running pure GDScript path.")
		extension_available = false

# =============================================================================
# Extension Access
# =============================================================================

## Get the Rust extension instance.
## Returns null if extension is not available.
func get_extension() -> RefCounted:
	return _extension


## Get a human-readable label for the loaded extension. Returns "N/A" when
## the extension is not loaded.
func get_extension_version() -> String:
	if _extension:
		return "sample_extension 0.1.0"
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
