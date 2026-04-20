@tool
extends "res://addons/powertool/commands/base_command.gd"


func process_command(peer_id: int, command_type: String, params: Dictionary, command_id: String) -> bool:
	match command_type:
		"get_editor_state":
			_get_editor_state(peer_id, params, command_id)
			return true
		"get_selected_node":
			_get_selected_node(peer_id, params, command_id)
			return true
		"execute_editor_script":
			_execute_editor_script(peer_id, params, command_id)
			return true
		"run_scene":
			_run_scene(peer_id, params, command_id)
			return true
		"stop_scene":
			_stop_scene(peer_id, params, command_id)
			return true
	return false


func _get_editor_state(peer_id: int, _params: Dictionary, command_id: String) -> void:
	var ei = _get_editor_interface()
	if not ei:
		return _send_error(peer_id, "EditorInterface not available", command_id)

	var root := _get_edited_scene_root()
	var scene_path := ""
	var root_type := ""
	if root:
		scene_path = root.scene_file_path
		root_type = root.get_class()

	var selected_nodes := []
	var selection = ei.get_selection()
	if selection:
		for node in selection.get_selected_nodes():
			selected_nodes.append({
				"name": node.name,
				"type": node.get_class(),
				"path": str(node.get_path()),
			})

	_send_success(peer_id, {
		"current_scene": scene_path,
		"root_type": root_type,
		"selected_nodes": selected_nodes,
		"is_playing": ei.is_playing_scene(),
		"project_path": ProjectSettings.globalize_path("res://"),
	}, command_id)


func _get_selected_node(peer_id: int, _params: Dictionary, command_id: String) -> void:
	var ei = _get_editor_interface()
	if not ei:
		return _send_error(peer_id, "EditorInterface not available", command_id)

	var selection = ei.get_selection()
	if not selection:
		return _send_success(peer_id, {"selected": false}, command_id)

	var nodes = selection.get_selected_nodes()
	if nodes.is_empty():
		return _send_success(peer_id, {"selected": false}, command_id)

	var node: Node = nodes[0]
	var props := {}
	for prop_name in ["position", "rotation", "scale", "size", "visible", "modulate", "z_index", "name"]:
		if prop_name in node:
			props[prop_name] = str(node.get(prop_name))

	_send_success(peer_id, {
		"selected": true,
		"name": node.name,
		"type": node.get_class(),
		"path": str(node.get_path()),
		"properties": props,
	}, command_id)


func _run_scene(peer_id: int, params: Dictionary, command_id: String) -> void:
	var ei = _get_editor_interface()
	if not ei:
		return _send_error(peer_id, "EditorInterface not available", command_id)

	if ei.is_playing_scene():
		return _send_error(peer_id, "A scene is already running. Stop it first with stop_scene.", command_id, "ALREADY_PLAYING")

	var scene_path: String = params.get("scene", "")
	if scene_path.is_empty():
		ei.play_main_scene()
	else:
		ei.play_custom_scene(scene_path)

	_send_success(peer_id, {
		"message": "Scene launched",
		"scene": scene_path if not scene_path.is_empty() else "(main scene)",
	}, command_id)


func _stop_scene(peer_id: int, _params: Dictionary, command_id: String) -> void:
	var ei = _get_editor_interface()
	if not ei:
		return _send_error(peer_id, "EditorInterface not available", command_id)

	if not ei.is_playing_scene():
		return _send_success(peer_id, {
			"message": "No scene was running",
		}, command_id)

	ei.stop_playing_scene()
	_send_success(peer_id, {
		"message": "Scene stopped",
	}, command_id)


func _execute_editor_script(peer_id: int, params: Dictionary, command_id: String) -> void:
	var code: String = params.get("code", "")

	if code.is_empty():
		return _send_error(peer_id, "code cannot be empty", command_id, "INVALID_PARAMS")

	var script_node := Node.new()
	script_node.name = "PowerToolScriptExecutor"
	add_child(script_node)

	var output := []
	var error_message := ""

	# Replace print() calls with custom_print() for output capture
	var modified_code := _replace_print_calls(code)

	# Indent user code for insertion into template
	var indented_lines := []
	for line in modified_code.split("\n"):
		# Normalize spaces to tabs
		var stripped := line
		var space_count := 0
		for i in range(line.length()):
			if line[i] == " ":
				space_count += 1
			else:
				break
		if space_count > 0:
			var tabs := ""
			@warning_ignore("integer_division")
			for _i in range(space_count / 4):
				tabs += "\t"
			stripped = tabs + line.substr(space_count)
		indented_lines.append("\t" + stripped)
	var indented_code := "\n".join(indented_lines)

	var script_content := """@tool
extends Node

signal execution_completed

var result = null
var _output_array = []
var _error_message = ""

func custom_print(values):
	var output_str = ""
	if values is Array:
		for i in range(values.size()):
			if i > 0:
				output_str += " "
			output_str += str(values[i])
	else:
		output_str = str(values)
	_output_array.append(output_str)

func run():
	var scene = get_tree().edited_scene_root
	var ret = _execute_code()
	if ret is int and ret != OK:
		_error_message = "Script execution failed with error: " + str(ret)
	elif not (ret is int):
		result = ret
	execution_completed.emit()

func _execute_code():
{user_code}
	return OK
"""
	script_content = script_content.replace("{user_code}", indented_code)

	var script := GDScript.new()
	script.source_code = script_content
	var err := script.reload()
	if err != OK:
		remove_child(script_node)
		script_node.queue_free()
		return _send_error(peer_id,
			"Script parse error: %s. User code is wrapped inside a function body — only statements are valid (no class definitions, extends, or func declarations)." % error_string(err),
			command_id, "INVALID_PARAMS", {
				"wrapped_source": script_content,
				"original_code": code,
				"hint": "Your code is inserted into _execute_code(). Only statements valid inside a function body are supported. Available variables: 'scene' (edited scene root).",
			})

	script_node.set_script(script)
	script_node.connect("execution_completed", _on_script_completed.bind(script_node, peer_id, command_id))
	script_node.run()


func _on_script_completed(script_node: Node, peer_id: int, command_id: String) -> void:
	var result = script_node.get("result")
	var output: Array = script_node._output_array
	var error_msg: String = script_node._error_message

	remove_child(script_node)
	script_node.queue_free()

	var result_data := {
		"success": error_msg.is_empty(),
		"output": output,
	}
	if not error_msg.is_empty():
		result_data["error"] = error_msg
	elif result != null:
		result_data["result"] = str(result)

	_send_success(peer_id, result_data, command_id)


func _replace_print_calls(code: String) -> String:
	var regex := RegEx.new()
	regex.compile("print\\s*\\(([^)]+)\\)")

	var matches := regex.search_all(code)
	var modified := code

	# Process in reverse to preserve string offsets
	for i in range(matches.size() - 1, -1, -1):
		var m := matches[i]
		var replacement := "custom_print([%s])" % m.get_string(1)
		modified = modified.substr(0, m.get_start()) + replacement + modified.substr(m.get_end())

	return modified
