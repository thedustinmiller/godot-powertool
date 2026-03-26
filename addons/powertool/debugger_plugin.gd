@tool
extends EditorDebuggerPlugin
## EditorDebuggerPlugin that communicates with the running game via Godot's
## debugger connection. Handles the "powertool:" message prefix.

signal game_response(message: String, data: Array)
signal game_started(session_id: int)
signal game_stopped(session_id: int)

var _active_session_id: int = -1


func _has_capture(prefix: String) -> bool:
	return prefix == "powertool"


func _capture(message: String, data: Array, session_id: int) -> bool:
	game_response.emit(message, data)
	return true


func _setup_session(session_id: int) -> void:
	var session := get_session(session_id)
	session.started.connect(_on_session_started.bind(session_id))
	session.stopped.connect(_on_session_stopped.bind(session_id))


func _on_session_started(session_id: int) -> void:
	_active_session_id = session_id
	print("[PowerTool] Game debugger session started: %d" % session_id)
	game_started.emit(session_id)


func _on_session_stopped(session_id: int) -> void:
	if _active_session_id == session_id:
		_active_session_id = -1
	print("[PowerTool] Game debugger session stopped: %d" % session_id)
	game_stopped.emit(session_id)


## Send a message to the running game. Returns true if a session was active.
func send_to_game(message: String, data: Array = []) -> bool:
	if _active_session_id < 0:
		return false
	var session := get_session(_active_session_id)
	if not session or not session.is_active():
		_active_session_id = -1
		return false
	session.send_message(message, data)
	return true


## Returns true if a game debugger session is currently active.
func is_game_running() -> bool:
	if _active_session_id < 0:
		return false
	var session := get_session(_active_session_id)
	return session != null and session.is_active()
