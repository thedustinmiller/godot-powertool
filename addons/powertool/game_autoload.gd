extends Node
## Game-side autoload that handles commands from the editor's PowerTool
## EditorDebuggerPlugin via the Godot debugger connection.

func _ready() -> void:
	if not EngineDebugger.is_active():
		return
	EngineDebugger.register_message_capture("powertool", _on_editor_message)
	print("[PowerTool] Game-side debugger capture registered")


func _on_editor_message(message: String, data: Array) -> bool:
	match message:
		"take_screenshot":
			_take_screenshot()
			return true
		"get_scene_tree":
			_send_scene_tree()
			return true
		"ping":
			EngineDebugger.send_message("powertool:pong", [])
			return true
	return false


func _take_screenshot() -> void:
	# Wait one frame for the viewport to have a current render
	await get_tree().process_frame

	var viewport := get_viewport()
	if not viewport:
		EngineDebugger.send_message("powertool:screenshot_error", ["No viewport available"])
		return

	var image := viewport.get_texture().get_image()
	if not image:
		EngineDebugger.send_message("powertool:screenshot_error", ["Failed to capture viewport"])
		return

	var png := image.save_png_to_buffer()
	if png.is_empty():
		EngineDebugger.send_message("powertool:screenshot_error", ["Failed to encode PNG"])
		return

	EngineDebugger.send_message("powertool:screenshot_ready", [
		png,
		image.get_width(),
		image.get_height(),
	])


func _send_scene_tree() -> void:
	var root := get_tree().current_scene
	if not root:
		EngineDebugger.send_message("powertool:scene_tree_error", ["No current scene"])
		return

	var tree := _build_tree(root)
	var json := JSON.stringify(tree)
	EngineDebugger.send_message("powertool:scene_tree_ready", [json])


func _build_tree(node: Node) -> Dictionary:
	var data := {
		"name": node.name,
		"type": node.get_class(),
	}
	var children := []
	for child in node.get_children():
		children.append(_build_tree(child))
	if not children.is_empty():
		data["children"] = children
	return data
