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
  "name": "Yagi-Uda antenna",
  "units": "mm",
  "params": [
    {
      "name": "l_driven",
      "expr": "12.4",
      "unit": "mm",
      "model": false
    },
    {
      "name": "l_reflector",
      "expr": "15.6",
      "unit": "mm",
      "model": false
    },
    {
      "name": "l_director",
      "expr": "12.6",
      "unit": "mm",
      "model": false
    },
    {
      "name": "s_reflector",
      "expr": "7.5",
      "unit": "mm",
      "model": false
    },
    {
      "name": "s_director",
      "expr": "6",
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
      "name": "Cylinder1",
      "suppressed": false,
      "body_names": [
        "driven1"
      ],
      "param_names": [
        "d1",
        "d2"
      ],
      "type": "cylinder",
      "base": [
        30.0,
        25.0,
        "25 - l_driven / 2"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "l_driven / 2 - 0.5",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder2",
      "suppressed": false,
      "body_names": [
        "driven2"
      ],
      "param_names": [
        "d3",
        "d4"
      ],
      "type": "cylinder",
      "base": [
        30.0,
        25.0,
        25.5
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "l_driven / 2 - 0.5",
      "operation": "new_body"
    },
    {
      "id": 3,
      "name": "Cylinder3",
      "suppressed": false,
      "body_names": [
        "reflector"
      ],
      "param_names": [
        "d5",
        "d6"
      ],
      "type": "cylinder",
      "base": [
        "30 - s_reflector",
        25.0,
        "25 - l_reflector / 2"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "l_reflector",
      "operation": "new_body"
    },
    {
      "id": 4,
      "name": "Cylinder4",
      "suppressed": false,
      "body_names": [
        "director"
      ],
      "param_names": [
        "d7",
        "d8"
      ],
      "type": "cylinder",
      "base": [
        "30 + s_director",
        25.0,
        "25 - l_director / 2"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "l_director",
      "operation": "new_body"
    }
  ],
  "next_id": 5,
  "next_component_color_index": 0
}