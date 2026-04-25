extends Control
## Main scene script.
##
## This is the entry point for the game. It demonstrates:
## - Accessing the GameManager autoload
## - Basic UI setup
## - (Optional) Using the Rust GDExtension when enabled

@onready var status_label: Label = $VBoxContainer/StatusLabel
@onready var version_label: Label = $VBoxContainer/VersionLabel
@onready var result_label: Label = $VBoxContainer/ResultLabel

func _ready() -> void:
	_update_status()

	# Connect to game manager signals
	GameManager.game_started.connect(_on_game_started)


func _update_status() -> void:
	if GameManager.has_extension():
		status_label.text = "Rust Extension: Loaded"
		status_label.add_theme_color_override("font_color", Color.GREEN)
		version_label.text = "Version: " + GameManager.get_extension_version()

		# Demonstrate calling into the Rust crate via SampleGreeter.
		var ext := GameManager.get_extension()
		if ext:
			result_label.text = "%s  fib(20) = %d (computed in Rust)" % [
				ext.greet("Godot"),
				ext.fibonacci(20),
			]
	else:
		status_label.text = "Ready (no extension)"
		version_label.text = ""
		result_label.text = ""


func _on_game_started() -> void:
	print("Game started!")


func _on_start_button_pressed() -> void:
	GameManager.start_game()


func _on_quit_button_pressed() -> void:
	get_tree().quit()
