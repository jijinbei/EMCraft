{
  "format": "solidcraft/51",
  "format_pending": [
    "bridged-counterbore",
    "embedded-surface-textures",
    "fillet-turn",
    "hole-direction-own-faces",
    "intersection-curve-sketch-curves",
    "point-cloud-segments-and-triangles",
    "revolve-axis-origin-expressions",
    "transferred-profile-frame",
    "two-sided-extrude-fixed-sides"
  ],
  "name": "Monopole",
  "units": "mm",
  "params": [
    {
      "name": "l",
      "expr": "7",
      "unit": "mm",
      "model": false
    },
    {
      "name": "ground",
      "expr": "40",
      "unit": "mm",
      "model": false
    },
    {
      "name": "r_wire",
      "expr": "0.5",
      "unit": "mm",
      "model": false
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "Box1",
      "suppressed": false,
      "body_names": [
        "ground"
      ],
      "param_names": [
        "d1",
        "d2",
        "d3"
      ],
      "type": "box",
      "corner": [
        "35 - ground / 2",
        "35 - ground / 2",
        19.0
      ],
      "length": "ground",
      "width": "ground",
      "height": "1",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder1",
      "suppressed": false,
      "body_names": [
        "wire"
      ],
      "param_names": [
        "d4",
        "d5"
      ],
      "type": "cylinder",
      "base": [
        35.0,
        35.0,
        20.5
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "l - 0.5",
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}