@tool
extends Node
## Routes incoming commands to processors and manages per-resource locking.

const LOCK_TIMEOUT_MS := 5000
const LOCK_SWEEP_INTERVAL := 1.0

var _server = null  # WebSocketServer reference
var _command_processors: Array = []
var _locks: Dictionary = {}  # resource_path -> {"peer_id": int, "acquired_at": int}
var _sweep_timer: float = 0.0

# Mutating commands that require a lock
const MUTATING_COMMANDS := [
	"create_node", "delete_node", "update_node_property",
	"create_scene", "open_scene", "save_scene",
	"create_script", "edit_script",
	"execute_editor_script",
]


func _ready() -> void:
	_init_processors()


func _process(delta: float) -> void:
	_sweep_timer += delta
	if _sweep_timer >= LOCK_SWEEP_INTERVAL:
		_sweep_timer = 0.0
		_sweep_expired_locks()


func set_server(server) -> void:
	_server = server
	for proc in _command_processors:
		proc._server = server


func _init_processors() -> void:
	var processor_scripts := [
		preload("res://addons/powertool/commands/node_commands.gd"),
		preload("res://addons/powertool/commands/scene_commands.gd"),
		preload("res://addons/powertool/commands/script_commands.gd"),
		preload("res://addons/powertool/commands/editor_commands.gd"),
		preload("res://addons/powertool/commands/screenshot_commands.gd"),
	]

	for script in processor_scripts:
		var proc := Node.new()
		proc.set_script(script)
		proc._server = _server
		add_child(proc)
		_command_processors.append(proc)


func _handle_command(peer_id: int, command: Dictionary) -> void:
	var command_id: String = str(command.get("id", ""))
	var command_type: String = str(command.get("command", ""))
	var params: Dictionary = command.get("params", {})

	if command_type.is_empty():
		_send_error(peer_id, "Missing 'command' field", command_id, "INVALID_PARAMS")
		return

	# Acquire lock for mutating commands
	var lock_key := _get_lock_key(command_type, params)
	if not lock_key.is_empty():
		if not _try_acquire_lock(lock_key, peer_id):
			_send_error(peer_id, "Resource is locked by another client: %s" % lock_key, command_id, "RESOURCE_LOCKED")
			return

	# Route to processors
	var handled := false
	for proc in _command_processors:
		if proc.process_command(peer_id, command_type, params, command_id):
			handled = true
			break

	if not handled:
		_send_error(peer_id, "Unknown command: %s" % command_type, command_id, "INVALID_PARAMS")

	# Always release lock after command completes
	if not lock_key.is_empty():
		_release_lock(lock_key)


func _get_lock_key(command_type: String, params: Dictionary) -> String:
	if command_type not in MUTATING_COMMANDS:
		return ""  # Read-only, no lock needed

	match command_type:
		"create_node", "delete_node", "update_node_property", "save_scene":
			# Lock the currently edited scene
			var plugin = Engine.get_meta("PowerToolPlugin", null)
			if plugin:
				var ei = plugin.get_editor_interface()
				var root = ei.get_edited_scene_root()
				if root:
					return root.scene_file_path
			return params.get("scene_path", params.get("path", "__current_scene__"))
		"create_scene", "open_scene":
			return params.get("path", "")
		"create_script", "edit_script":
			return params.get("script_path", "")
		"execute_editor_script":
			return "__editor_script__"

	return ""


func _try_acquire_lock(resource_path: String, peer_id: int) -> bool:
	if resource_path in _locks:
		var lock: Dictionary = _locks[resource_path]
		if lock["peer_id"] == peer_id:
			# Same peer, refresh timestamp
			lock["acquired_at"] = Time.get_ticks_msec()
			return true
		# Different peer — check if expired
		if lock["acquired_at"] + LOCK_TIMEOUT_MS < Time.get_ticks_msec():
			# Expired, take over
			_locks[resource_path] = {"peer_id": peer_id, "acquired_at": Time.get_ticks_msec()}
			return true
		return false  # Locked by another active peer

	_locks[resource_path] = {"peer_id": peer_id, "acquired_at": Time.get_ticks_msec()}
	return true


func _release_lock(resource_path: String) -> void:
	_locks.erase(resource_path)


func _release_all_locks(peer_id: int) -> void:
	var to_release := []
	for path in _locks:
		if _locks[path]["peer_id"] == peer_id:
			to_release.append(path)
	for path in to_release:
		_locks.erase(path)


func _sweep_expired_locks() -> void:
	var now := Time.get_ticks_msec()
	var to_remove := []
	for path in _locks:
		if _locks[path]["acquired_at"] + LOCK_TIMEOUT_MS < now:
			to_remove.append(path)
	for path in to_remove:
		_locks.erase(path)


func _on_client_connected(peer_id: int) -> void:
	print("[PowerTool] Client connected: %d" % peer_id)


func _on_client_disconnected(peer_id: int) -> void:
	print("[PowerTool] Client disconnected: %d" % peer_id)
	_release_all_locks(peer_id)


func _send_error(peer_id: int, message: String, command_id: String, code: String) -> void:
	if _server:
		_server.send_response(peer_id, {
			"id": command_id,
			"status": "error",
			"message": message,
			"code": code,
		})
