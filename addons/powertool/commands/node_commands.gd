@tool
extends "res://addons/powertool/commands/base_command.gd"


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	match command_type:
		"create_node":
			_create_node(peer_id, params, command_id)
			return true
		"delete_node":
			_delete_node(peer_id, params, command_id)
			return true
		"update_node_property":
			_update_node_property(peer_id, params, command_id)
			return true
		"get_node_properties":
			_get_node_properties(peer_id, params, command_id)
			return true
		"list_nodes":
			_list_nodes(peer_id, params, command_id)
			return true
	return false


func _create_node(peer_id: int, params: Dictionary, command_id: String) -> void:
	var parent_path: String = params.get("parent_path", "/root")
	var node_type: String = params.get("node_type", "Node")
	var node_name: String = params.get("node_name", "NewNode")

	if not ClassDB.class_exists(node_type):
		return _send_error(peer_id, "Invalid node type: %s" % node_type, command_id, "INVALID_PARAMS")

	if not ClassDB.can_instantiate(node_type):
		return _send_error(peer_id, "Cannot instantiate type: %s" % node_type, command_id, "INVALID_PARAMS")

	var root := _get_edited_scene_root()
	if not root:
		return _send_error(peer_id, "No scene is currently being edited", command_id, "NO_SCENE")

	var parent := _get_editor_node(parent_path)
	if not parent:
		return _send_error(peer_id, "Parent node not found: %s" % parent_path, command_id, "INVALID_PARAMS", _node_not_found_details(parent_path))

	var node = ClassDB.instantiate(node_type)
	if not node:
		return _send_error(peer_id, "Failed to create node of type: %s" % node_type, command_id)

	node.name = node_name
	parent.add_child(node)
	node.owner = root

	# Apply initial properties if provided
	var properties: Dictionary = params.get("properties", {})
	for prop_name in properties:
		if prop_name in node:
			node.set(prop_name, _parse_property_value(properties[prop_name]))

	_mark_scene_modified()

	_send_success(peer_id, {
		"node_path": str(node.get_path()),
		"node_type": node_type,
		"node_name": node.name,
	}, command_id)


func _delete_node(peer_id: int, params: Dictionary, command_id: String) -> void:
	var node_path: String = params.get("node_path", "")

	if node_path.is_empty():
		return _send_error(peer_id, "node_path cannot be empty", command_id, "INVALID_PARAMS")

	var root := _get_edited_scene_root()
	if not root:
		return _send_error(peer_id, "No scene is currently being edited", command_id, "NO_SCENE")

	var node := _get_editor_node(node_path)
	if not node:
		return _send_error(peer_id, "Node not found: %s" % node_path, command_id, "INVALID_PARAMS", _node_not_found_details(node_path))

	if node == root:
		return _send_error(peer_id, "Cannot delete the root node", command_id, "INVALID_PARAMS")

	var parent := node.get_parent()
	parent.remove_child(node)
	node.queue_free()
	_mark_scene_modified()

	_send_success(peer_id, {"deleted_node_path": node_path}, command_id)


func _update_node_property(peer_id: int, params: Dictionary, command_id: String) -> void:
	var node_path: String = params.get("node_path", "")
	var property_name: String = params.get("property", "")
	var property_value = params.get("value")

	if node_path.is_empty():
		return _send_error(peer_id, "node_path cannot be empty", command_id, "INVALID_PARAMS")
	if property_name.is_empty():
		return _send_error(peer_id, "property cannot be empty", command_id, "INVALID_PARAMS")
	if property_value == null:
		return _send_error(peer_id, "value cannot be null", command_id, "INVALID_PARAMS")

	var node := _get_editor_node(node_path)
	if not node:
		return _send_error(peer_id, "Node not found: %s" % node_path, command_id, "INVALID_PARAMS", _node_not_found_details(node_path))

	if not property_name in node:
		return _send_error(peer_id, "Property '%s' does not exist on node %s" % [property_name, node_path], command_id, "INVALID_PARAMS")

	var parsed_value = _parse_property_value(property_value)
	var old_value = node.get(property_name)

	var undo_redo = _get_undo_redo()
	if undo_redo:
		undo_redo.create_action("Update %s.%s" % [node.name, property_name])
		undo_redo.add_do_property(node, property_name, parsed_value)
		undo_redo.add_undo_property(node, property_name, old_value)
		undo_redo.commit_action()
	else:
		node.set(property_name, parsed_value)

	_mark_scene_modified()

	_send_success(peer_id, {
		"node_path": node_path,
		"property": property_name,
		"value": str(parsed_value),
	}, command_id)


func _get_node_properties(peer_id: int, params: Dictionary, command_id: String) -> void:
	var node_path: String = params.get("node_path", "")

	if node_path.is_empty():
		return _send_error(peer_id, "node_path cannot be empty", command_id, "INVALID_PARAMS")

	var node := _get_editor_node(node_path)
	if not node:
		return _send_error(peer_id, "Node not found: %s" % node_path, command_id, "INVALID_PARAMS", _node_not_found_details(node_path))

	var properties := {}
	for prop in node.get_property_list():
		var name: String = prop["name"]
		if not name.begins_with("_"):
			var val = node.get(name)
			# Convert to string for JSON safety
			properties[name] = str(val) if typeof(val) not in [TYPE_NIL, TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING] else val

	_send_success(peer_id, {
		"node_path": node_path,
		"node_type": node.get_class(),
		"properties": properties,
	}, command_id)


func _list_nodes(peer_id: int, params: Dictionary, command_id: String) -> void:
	var parent_path: String = params.get("parent_path", "/root")

	var parent := _get_editor_node(parent_path)
	if not parent:
		return _send_error(peer_id, "Parent node not found: %s" % parent_path, command_id, "INVALID_PARAMS", _node_not_found_details(parent_path))

	var children := []
	for child in parent.get_children():
		children.append({
			"name": child.name,
			"type": child.get_class(),
			"path": str(child.get_path()),
		})

	_send_success(peer_id, {
		"parent_path": parent_path,
		"children": children,
	}, command_id)
