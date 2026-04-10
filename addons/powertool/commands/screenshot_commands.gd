@tool
extends "res://addons/powertool/commands/base_command.gd"

## Reference to the debugger plugin, set by command_handler
var _debugger = null

const SCREENSHOT_TIMEOUT := 10.0


func set_debugger_ref(debugger) -> void:
	_debugger = debugger


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	match command_type:
		"take_screenshot":
			_take_screenshot(peer_id, params, command_id)
			return true
		"pause_for_screenshot":
			_pause_game(peer_id, command_id)
			return true
		"resume_after_screenshot":
			_resume_game(peer_id, command_id)
			return true
	return false


func _take_screenshot(peer_id: int, params: Dictionary, command_id: String) -> void:
	var source: String = params.get("source", "auto")

	# "auto" picks running game if available, else editor viewport
	# "game" forces running game, "editor" forces editor viewport
	if source == "auto":
		if _debugger and _debugger.is_game_running():
			source = "game"
		else:
			source = "editor"

	if source == "game":
		_take_game_screenshot(peer_id, command_id)
	else:
		_take_editor_screenshot(peer_id, command_id)


func _take_game_screenshot(peer_id: int, command_id: String) -> void:
	if not _debugger or not _debugger.is_game_running():
		return _send_error(peer_id, "No game is currently running", command_id, "NO_SCENE")

	# Send request to game via debugger
	if not _debugger.send_to_game("powertool:take_screenshot"):
		return _send_error(peer_id, "Failed to send screenshot request to game", command_id)

	# Wait for response with timeout
	var result = await _wait_for_game_response(["powertool:screenshot_ready", "powertool:screenshot_error"])

	if result == null:
		return _send_error(peer_id, "Screenshot request timed out", command_id)

	if result["message"] == "powertool:screenshot_error":
		var err_msg: String = result["data"][0] if result["data"].size() > 0 else "Unknown error"
		return _send_error(peer_id, "Game screenshot failed: %s" % err_msg, command_id)

	# screenshot_ready: [png_bytes, width, height]
	var png_data: PackedByteArray = result["data"][0]
	var width: int = result["data"][1]
	var height: int = result["data"][2]

	var base64 := Marshalls.raw_to_base64(png_data)
	_send_success(peer_id, {
		"image_base64": base64,
		"format": "png",
		"width": width,
		"height": height,
		"source": "game",
	}, command_id)


func _take_editor_screenshot(peer_id: int, command_id: String) -> void:
	var root := _get_edited_scene_root()
	if not root:
		return _send_error(peer_id, "No scene is currently being edited", command_id, "NO_SCENE")

	var viewport := root.get_viewport()
	if not viewport:
		return _send_error(peer_id, "No viewport available", command_id)

	await get_tree().process_frame

	var image := viewport.get_texture().get_image()
	if not image:
		return _send_error(peer_id, "Failed to capture viewport image", command_id)

	var png_buffer := image.save_png_to_buffer()
	if png_buffer.is_empty():
		return _send_error(peer_id, "Failed to encode image as PNG", command_id)

	var base64 := Marshalls.raw_to_base64(png_buffer)
	_send_success(peer_id, {
		"image_base64": base64,
		"format": "png",
		"width": image.get_width(),
		"height": image.get_height(),
		"source": "editor",
	}, command_id)


## Await a game response matching one of the expected messages, with timeout.
func _wait_for_game_response(expected_messages: Array) -> Variant:
	# Use an Array as a box so the lambda can mutate shared state
	# (GDScript lambdas capture primitives by value)
	var state := [null, false]  # [result, received]

	var callback := func(message: String, data: Array):
		if message in expected_messages:
			state[0] = {"message": message, "data": data}
			state[1] = true

	_debugger.game_response.connect(callback)

	# Poll until received or timeout
	var elapsed := 0.0
	while not state[1] and elapsed < SCREENSHOT_TIMEOUT:
		await get_tree().create_timer(0.05).timeout
		elapsed += 0.05

	if _debugger.game_response.is_connected(callback):
		_debugger.game_response.disconnect(callback)

	return state[0]


func _pause_game(peer_id: int, command_id: String) -> void:
	if not _debugger or not _debugger.is_game_running():
		return _send_error(peer_id, "No game is currently running", command_id, "NO_SCENE")

	if not _debugger.send_to_game("powertool:pause"):
		return _send_error(peer_id, "Failed to send pause request to game", command_id)

	var result = await _wait_for_game_response(["powertool:paused"])
	if result == null:
		return _send_error(peer_id, "Pause request timed out", command_id)

	_send_success(peer_id, {"paused": true}, command_id)


func _resume_game(peer_id: int, command_id: String) -> void:
	if not _debugger or not _debugger.is_game_running():
		return _send_error(peer_id, "No game is currently running", command_id, "NO_SCENE")

	if not _debugger.send_to_game("powertool:resume"):
		return _send_error(peer_id, "Failed to send resume request to game", command_id)

	var result = await _wait_for_game_response(["powertool:resumed"])
	if result == null:
		return _send_error(peer_id, "Resume request timed out", command_id)

	_send_success(peer_id, {"paused": false}, command_id)
