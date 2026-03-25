@tool
extends Node
## WebSocket server for PowerTool MCP communication.
##
## Based on the official Godot WebSocket demo pattern (TCPServer + WebSocketPeer).
## Parses incoming text messages as JSON and emits command_received.

signal command_received(peer_id: int, command: Dictionary)
signal client_connected(peer_id: int)
signal client_disconnected(peer_id: int)

@export var handshake_timeout := 3000
@export var supported_protocols := PackedStringArray()


class PendingPeer:
	var connect_time: int
	var tcp: StreamPeerTCP
	var connection: StreamPeer
	var ws: WebSocketPeer

	func _init(p_tcp: StreamPeerTCP) -> void:
		tcp = p_tcp
		connection = p_tcp
		connect_time = Time.get_ticks_msec()


var tcp_server := TCPServer.new()
var pending_peers: Array[PendingPeer] = []
var peers: Dictionary = {}


func start_server(port: int) -> int:
	assert(not tcp_server.is_listening())
	var err := tcp_server.listen(port, "127.0.0.1")
	if err == OK:
		print("[PowerTool] WebSocket server listening on 127.0.0.1:%d" % port)
	else:
		push_error("[PowerTool] Failed to listen on port %d: %s" % [port, error_string(err)])
	return err


func stop_server() -> void:
	tcp_server.stop()
	pending_peers.clear()
	peers.clear()


func send_response(peer_id: int, response: Dictionary) -> int:
	if not peers.has(peer_id):
		return ERR_DOES_NOT_EXIST
	var json := JSON.stringify(response)
	return peers[peer_id].send_text(json)


func poll() -> void:
	if not tcp_server.is_listening():
		return

	# Accept new TCP connections
	while tcp_server.is_connection_available():
		var conn: StreamPeerTCP = tcp_server.take_connection()
		assert(conn != null)
		pending_peers.append(PendingPeer.new(conn))

	# Process pending handshakes
	var to_remove := []
	for p in pending_peers:
		if not _connect_pending(p):
			if p.connect_time + handshake_timeout < Time.get_ticks_msec():
				to_remove.append(p)
			continue
		to_remove.append(p)

	for r: RefCounted in to_remove:
		pending_peers.erase(r)
	to_remove.clear()

	# Poll connected peers
	for id: int in peers:
		var p: WebSocketPeer = peers[id]
		p.poll()

		if p.get_ready_state() != WebSocketPeer.STATE_OPEN:
			client_disconnected.emit(id)
			to_remove.append(id)
			continue

		while p.get_available_packet_count():
			var pkt: PackedByteArray = p.get_packet()
			if p.was_string_packet():
				_handle_text_message(id, pkt.get_string_from_utf8())

	for r: int in to_remove:
		peers.erase(r)
	to_remove.clear()


func _handle_text_message(peer_id: int, text: String) -> void:
	var json := JSON.new()
	var err := json.parse(text)
	if err != OK:
		send_response(peer_id, {
			"id": "",
			"status": "error",
			"message": "Invalid JSON: %s" % json.get_error_message(),
			"code": "INVALID_JSON",
		})
		return

	var data: Variant = json.data
	if typeof(data) != TYPE_DICTIONARY:
		send_response(peer_id, {
			"id": "",
			"status": "error",
			"message": "Expected JSON object",
			"code": "INVALID_JSON",
		})
		return

	command_received.emit(peer_id, data as Dictionary)


func _create_peer() -> WebSocketPeer:
	var ws := WebSocketPeer.new()
	ws.supported_protocols = supported_protocols
	return ws


func _connect_pending(p: PendingPeer) -> bool:
	if p.ws != null:
		p.ws.poll()
		var state := p.ws.get_ready_state()
		if state == WebSocketPeer.STATE_OPEN:
			var id := randi_range(2, 1 << 30)
			peers[id] = p.ws
			client_connected.emit(id)
			return true
		elif state != WebSocketPeer.STATE_CONNECTING:
			return true  # Handshake failed
		return false  # Still connecting
	elif p.tcp.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		return true  # TCP disconnected
	else:
		p.ws = _create_peer()
		p.ws.accept_stream(p.tcp)
		return false  # WebSocket handshake pending


func _process(_delta: float) -> void:
	poll()
