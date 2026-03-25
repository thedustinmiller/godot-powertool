@tool
extends "res://addons/powertool/commands/base_command.gd"


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	match command_type:
		"create_script":
			_create_script(peer_id, params, command_id)
			return true
		"edit_script":
			_edit_script(peer_id, params, command_id)
			return true
		"get_script":
			_get_script(peer_id, params, command_id)
			return true
	return false


func _create_script(peer_id: int, params: Dictionary, command_id: String) -> void:
	var script_path: String = params.get("script_path", "")
	var content: String = params.get("content", "")
	var node_path: String = params.get("node_path", "")

	if script_path.is_empty():
		return _send_error(peer_id, "script_path cannot be empty", command_id, "INVALID_PARAMS")

	if not script_path.begins_with("res://"):
		script_path = "res://" + script_path
	if not script_path.ends_with(".gd"):
		script_path += ".gd"

	# Provide a default script template if no content given
	if content.is_empty():
		content = "extends Node\n\n\nfunc _ready() -> void:\n\tpass\n"

	# Ensure directory exists
	var dir_path := script_path.get_base_dir()
	if not DirAccess.dir_exists_absolute(dir_path):
		DirAccess.make_dir_recursive_absolute(dir_path)

	var file := FileAccess.open(script_path, FileAccess.WRITE)
	if not file:
		return _send_error(peer_id, "Failed to open file for writing: %s" % script_path, command_id)

	file.store_string(content)
	file = null  # Close file

	# Refresh filesystem so editor picks it up
	var ei = _get_editor_interface()
	if ei:
		ei.get_resource_filesystem().scan()

	# Optionally attach to a node
	if not node_path.is_empty():
		# Wait briefly for filesystem scan
		await get_tree().create_timer(0.3).timeout
		var node := _get_editor_node(node_path)
		if node:
			var script := load(script_path) as Script
			if script:
				node.set_script(script)
				_mark_scene_modified()

	_send_success(peer_id, {"script_path": script_path}, command_id)


func _edit_script(peer_id: int, params: Dictionary, command_id: String) -> void:
	var script_path: String = params.get("script_path", "")
	var content: String = params.get("content", "")

	if script_path.is_empty():
		return _send_error(peer_id, "script_path cannot be empty", command_id, "INVALID_PARAMS")
	if content.is_empty():
		return _send_error(peer_id, "content cannot be empty", command_id, "INVALID_PARAMS")

	if not script_path.begins_with("res://"):
		script_path = "res://" + script_path

	if not FileAccess.file_exists(script_path):
		return _send_error(peer_id, "Script file not found: %s" % script_path, command_id, "INVALID_PARAMS")

	var file := FileAccess.open(script_path, FileAccess.WRITE)
	if not file:
		return _send_error(peer_id, "Failed to open file for writing: %s" % script_path, command_id)

	file.store_string(content)
	file = null

	var ei = _get_editor_interface()
	if ei:
		ei.get_resource_filesystem().scan()

	_send_success(peer_id, {"script_path": script_path}, command_id)


func _get_script(peer_id: int, params: Dictionary, command_id: String) -> void:
	var script_path: String = params.get("script_path", "")
	var node_path: String = params.get("node_path", "")

	# If node_path provided, get script from node
	if not node_path.is_empty():
		var node := _get_editor_node(node_path)
		if not node:
			return _send_error(peer_id, "Node not found: %s" % node_path, command_id, "INVALID_PARAMS")
		var script = node.get_script()
		if not script:
			return _send_error(peer_id, "Node has no script: %s" % node_path, command_id, "INVALID_PARAMS")
		script_path = script.resource_path

	if script_path.is_empty():
		return _send_error(peer_id, "script_path or node_path required", command_id, "INVALID_PARAMS")

	if not script_path.begins_with("res://"):
		script_path = "res://" + script_path

	if not FileAccess.file_exists(script_path):
		return _send_error(peer_id, "Script file not found: %s" % script_path, command_id, "INVALID_PARAMS")

	var file := FileAccess.open(script_path, FileAccess.READ)
	if not file:
		return _send_error(peer_id, "Failed to read file: %s" % script_path, command_id)

	var content := file.get_as_text()
	file = null

	_send_success(peer_id, {"script_path": script_path, "content": content}, command_id)
