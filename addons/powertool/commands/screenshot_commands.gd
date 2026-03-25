@tool
extends "res://addons/powertool/commands/base_command.gd"


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	match command_type:
		"take_screenshot":
			_take_screenshot(peer_id, params, command_id)
			return true
	return false


func _take_screenshot(peer_id: int, _params: Dictionary, command_id: String) -> void:
	var root := _get_edited_scene_root()
	if not root:
		return _send_error(peer_id, "No scene is currently being edited", command_id, "NO_SCENE")

	var viewport := root.get_viewport()
	if not viewport:
		return _send_error(peer_id, "No viewport available", command_id)

	# Wait one frame to ensure the viewport has rendered
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
	}, command_id)
