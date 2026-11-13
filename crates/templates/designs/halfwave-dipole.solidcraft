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
  "name": "Half-wave dipole",
  "units": "mm",
  "params": [
    {
      "name": "arm",
      "expr": "7",
      "unit": "mm",
      "model": false
    },
    {
      "name": "gap",
      "expr": "1",
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
        "arm1"
      ],
      "param_names": [
        "d1",
        "d2"
      ],
      "type": "cylinder",
      "base": [
        30.0,
        30.0,
        "30 - gap / 2 - arm"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "arm",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder2",
      "suppressed": false,
      "body_names": [
        "arm2"
      ],
      "param_names": [
        "d3",
        "d4"
      ],
      "type": "cylinder",
      "base": [
        30.0,
        30.0,
        "30 + gap / 2"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_wire",
      "height": "arm",
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}