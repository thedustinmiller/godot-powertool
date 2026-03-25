@tool
class_name PowerToolNodeUtils
## Static utility functions for node operations.


static func node_to_dict(node: Node, include_properties: bool = false) -> Dictionary:
	var data := {
		"name": node.name,
		"type": node.get_class(),
		"path": str(node.get_path()),
	}

	if include_properties:
		var props := {}
		for prop in node.get_property_list():
			var name: String = prop["name"]
			if not name.begins_with("_") and name in ["position", "rotation", "scale", "size", "visible", "modulate", "z_index"]:
				props[name] = str(node.get(name))
		data["properties"] = props

	if node.get_child_count() > 0:
		data["child_count"] = node.get_child_count()

	return data


static func get_nodes_by_type(root: Node, type_name: String) -> Array[Node]:
	var result: Array[Node] = []
	_collect_by_type(root, type_name, result)
	return result


static func _collect_by_type(node: Node, type_name: String, result: Array[Node]) -> void:
	if node.is_class(type_name):
		result.append(node)
	for child in node.get_children():
		_collect_by_type(child, type_name, result)


static func build_scene_tree(node: Node) -> Dictionary:
	var data := node_to_dict(node, true)
	var children := []
	for child in node.get_children():
		children.append(build_scene_tree(child))
	if children.size() > 0:
		data["children"] = children
	return data
