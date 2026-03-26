@tool
extends Node
## Base command processor for PowerTool.
## All command processors extend this and override process_command().

var _server = null  # WebSocketServer reference, set by CommandHandler


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	return false


func _send_success(peer_id: int, result: Dictionary, command_id: String) -> void:
	if _server:
		_server.send_response(peer_id, {
			"id": command_id,
			"status": "success",
			"result": result,
		})


func _send_error(peer_id: int, message: String, command_id: String, code: String = "INTERNAL_ERROR", details: Dictionary = {}) -> void:
	if _server:
		var response := {
			"id": command_id,
			"status": "error",
			"message": message,
			"code": code,
		}
		if not details.is_empty():
			response["details"] = details
		_server.send_response(peer_id, response)
	push_error("[PowerTool] %s" % message)


func _get_plugin():
	return Engine.get_meta("PowerToolPlugin", null)


func _get_editor_interface():
	var plugin = _get_plugin()
	if plugin:
		return plugin.get_editor_interface()
	return null


func _get_edited_scene_root() -> Node:
	var ei = _get_editor_interface()
	if ei:
		return ei.get_edited_scene_root()
	return null


func _get_editor_node(path: String) -> Node:
	var root := _get_edited_scene_root()
	if not root:
		return null

	if path == "/root" or path == "" or path == ".":
		return root

	if path.begins_with("/root/"):
		path = path.substr(6)
	elif path.begins_with("/"):
		path = path.substr(1)

	return root.get_node_or_null(path)


## Build details dict for a failed node path lookup.
func _node_not_found_details(path: String) -> Dictionary:
	var root := _get_edited_scene_root()
	if not root:
		return {"hint": "No scene is currently being edited."}

	var details := {
		"scene_root_name": root.name,
		"scene_root_type": root.get_class(),
		"requested_path": path,
	}

	# Find the deepest valid ancestor to show its children
	var resolved_path := path
	if resolved_path.begins_with("/root/"):
		resolved_path = resolved_path.substr(6)
	elif resolved_path.begins_with("/"):
		resolved_path = resolved_path.substr(1)

	# Check if the first segment matches the scene root name — common mistake
	var segments := resolved_path.split("/")
	if segments.size() > 0 and segments[0] == root.name:
		details["hint"] = "The scene root '%s' is addressed as /root, not /root/%s. Try /root/%s instead." % [
			root.name, root.name, "/".join(segments.slice(1))
		]

	# List children of the closest valid ancestor
	var parent := root
	var valid_depth := 0
	for i in range(segments.size() - 1):
		var child := parent.get_node_or_null(segments[i])
		if child:
			parent = child
			valid_depth = i + 1
		else:
			break

	var child_names: Array[String] = []
	for child in parent.get_children():
		child_names.append(child.name)
	details["nearest_valid_path"] = "/root" if valid_depth == 0 else "/root/" + "/".join(segments.slice(0, valid_depth))
	details["available_children"] = child_names

	return details


func _mark_scene_modified() -> void:
	var ei = _get_editor_interface()
	if ei:
		ei.mark_scene_as_unsaved()


func _get_undo_redo():
	var plugin = _get_plugin()
	if plugin and plugin.has_method("get_undo_redo"):
		return plugin.get_undo_redo()
	return null


func _parse_property_value(value):
	if typeof(value) != TYPE_STRING:
		return value

	# Try to parse Godot type constructors like "Vector2(1, 2)"
	var type_prefixes := [
		"Vector", "Transform", "Rect", "Color", "Quat", "Basis",
		"Plane", "AABB", "Projection", "PackedVector", "PackedString",
		"PackedFloat", "PackedInt", "PackedColor", "PackedByteArray",
	]

	var is_type := false
	for prefix in type_prefixes:
		if value.begins_with(prefix):
			is_type = true
			break

	if not is_type:
		return value

	var expression := Expression.new()
	var err := expression.parse(value, [])
	if err == OK:
		var result = expression.execute([], null, true)
		if not expression.has_execute_failed():
			return result

	return value
