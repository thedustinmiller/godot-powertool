@tool
extends "res://addons/powertool/commands/base_command.gd"


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	match command_type:
		"create_scene":
			_create_scene(peer_id, params, command_id)
			return true
		"open_scene":
			_open_scene(peer_id, params, command_id)
			return true
		"save_scene":
			_save_scene(peer_id, params, command_id)
			return true
		"get_current_scene":
			_get_current_scene(peer_id, params, command_id)
			return true
		"get_scene_structure":
			_get_scene_structure(peer_id, params, command_id)
			return true
	return false


func _create_scene(peer_id: int, params: Dictionary, command_id: String) -> void:
	var path: String = params.get("path", "")
	var root_type: String = params.get("root_type", "Node2D")

	if path.is_empty():
		return _send_error(peer_id, "path cannot be empty", command_id, "INVALID_PARAMS")

	if not path.begins_with("res://"):
		path = "res://" + path
	if not path.ends_with(".tscn"):
		path += ".tscn"

	if not ClassDB.class_exists(root_type):
		return _send_error(peer_id, "Invalid root type: %s" % root_type, command_id, "INVALID_PARAMS")

	# Ensure directory exists
	var dir_path := path.get_base_dir()
	if not DirAccess.dir_exists_absolute(dir_path):
		DirAccess.make_dir_recursive_absolute(dir_path)

	var root_node = ClassDB.instantiate(root_type)
	if not root_node:
		return _send_error(peer_id, "Cannot instantiate: %s" % root_type, command_id)

	root_node.name = path.get_file().get_basename()

	var packed := PackedScene.new()
	var err := packed.pack(root_node)
	root_node.queue_free()

	if err != OK:
		return _send_error(peer_id, "Failed to pack scene: %s" % error_string(err), command_id)

	err = ResourceSaver.save(packed, path)
	if err != OK:
		return _send_error(peer_id, "Failed to save scene: %s" % error_string(err), command_id)

	# Open in editor
	var ei = _get_editor_interface()
	if ei:
		ei.get_resource_filesystem().scan()
		ei.open_scene_from_path(path)

	_send_success(peer_id, {"scene_path": path, "root_type": root_type}, command_id)


func _open_scene(peer_id: int, params: Dictionary, command_id: String) -> void:
	var path: String = params.get("path", "")

	if path.is_empty():
		return _send_error(peer_id, "path cannot be empty", command_id, "INVALID_PARAMS")

	if not path.begins_with("res://"):
		path = "res://" + path

	if not FileAccess.file_exists(path):
		return _send_error(peer_id, "Scene file not found: %s" % path, command_id, "INVALID_PARAMS")

	var ei = _get_editor_interface()
	if not ei:
		return _send_error(peer_id, "EditorInterface not available", command_id)

	ei.open_scene_from_path(path)

	_send_success(peer_id, {"scene_path": path}, command_id)


func _save_scene(peer_id: int, params: Dictionary, command_id: String) -> void:
	var path: String = params.get("path", "")

	var ei = _get_editor_interface()
	if not ei:
		return _send_error(peer_id, "EditorInterface not available", command_id)

	var root := _get_edited_scene_root()
	if not root:
		return _send_error(peer_id, "No scene is currently being edited", command_id, "NO_SCENE")

	if path.is_empty():
		path = root.scene_file_path

	if path.is_empty():
		return _send_error(peer_id, "Scene has no path and none was provided", command_id, "INVALID_PARAMS")

	if not path.begins_with("res://"):
		path = "res://" + path
	if not path.ends_with(".tscn"):
		path += ".tscn"

	var packed := PackedScene.new()
	var err := packed.pack(root)
	if err != OK:
		return _send_error(peer_id, "Failed to pack scene: %s" % error_string(err), command_id)

	err = ResourceSaver.save(packed, path)
	if err != OK:
		return _send_error(peer_id, "Failed to save scene: %s" % error_string(err), command_id)

	_send_success(peer_id, {"scene_path": path}, command_id)


func _get_current_scene(peer_id: int, _params: Dictionary, command_id: String) -> void:
	var root := _get_edited_scene_root()
	if not root:
		_send_success(peer_id, {"scene_path": "", "root_type": "", "root_name": ""}, command_id)
		return

	_send_success(peer_id, {
		"scene_path": root.scene_file_path,
		"root_type": root.get_class(),
		"root_name": root.name,
	}, command_id)


func _get_scene_structure(peer_id: int, _params: Dictionary, command_id: String) -> void:
	var root := _get_edited_scene_root()
	if not root:
		return _send_error(peer_id, "No scene is currently being edited", command_id, "NO_SCENE")

	var structure := PowerToolNodeUtils.build_scene_tree(root)
	_send_success(peer_id, {"scene_path": root.scene_file_path, "tree": structure}, command_id)
