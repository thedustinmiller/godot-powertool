@tool
extends EditorPlugin
## PowerTool EditorPlugin — WebSocket bridge for AI-powered Godot development.

const DEFAULT_PORT := 6550
const GAME_AUTOLOAD_NAME := "PowerToolGame"
const GAME_AUTOLOAD_PATH := "res://addons/powertool/game_autoload.gd"

var _ws_server: Node = null
var _command_handler: Node = null
var _debugger_plugin: EditorDebuggerPlugin = null


func _enter_tree() -> void:
	Engine.set_meta("PowerToolPlugin", self)

	var port := DEFAULT_PORT
	var env_port := OS.get_environment("POWERTOOL_PORT")
	if not env_port.is_empty() and env_port.is_valid_int():
		port = int(env_port)

	# Create WebSocket server
	var ws_script := preload("res://addons/powertool/websocket_server.gd")
	_ws_server = Node.new()
	_ws_server.set_script(ws_script)
	_ws_server.name = "PowerToolWebSocket"
	add_child(_ws_server)

	# Create command handler
	var handler_script := preload("res://addons/powertool/command_handler.gd")
	_command_handler = Node.new()
	_command_handler.set_script(handler_script)
	_command_handler.name = "PowerToolCommandHandler"
	add_child(_command_handler)

	# Register debugger plugin for game communication
	_debugger_plugin = preload("res://addons/powertool/debugger_plugin.gd").new()
	add_debugger_plugin(_debugger_plugin)

	# Wire command handler to server and debugger (deferred to ensure _ready has run)
	_command_handler.set_server.call_deferred(_ws_server)
	_command_handler.set_debugger.call_deferred(_debugger_plugin)

	# Connect signals
	_ws_server.command_received.connect(_command_handler._handle_command)
	_ws_server.client_connected.connect(_command_handler._on_client_connected)
	_ws_server.client_disconnected.connect(_command_handler._on_client_disconnected)

	# Register game-side autoload so the running game can talk back
	if not ProjectSettings.has_setting("autoload/" + GAME_AUTOLOAD_NAME):
		add_autoload_singleton(GAME_AUTOLOAD_NAME, GAME_AUTOLOAD_PATH)

	# Start listening
	_ws_server.start_server(port)


func _exit_tree() -> void:
	if _ws_server:
		_ws_server.stop_server()

	if _debugger_plugin:
		remove_debugger_plugin(_debugger_plugin)
		_debugger_plugin = null

	if ProjectSettings.has_setting("autoload/" + GAME_AUTOLOAD_NAME):
		remove_autoload_singleton(GAME_AUTOLOAD_NAME)

	Engine.remove_meta("PowerToolPlugin")
