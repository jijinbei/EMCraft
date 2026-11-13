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
  "name": "Dielectric resonator",
  "units": "mm",
  "params": [
    {
      "name": "r_puck",
      "expr": "5.25",
      "unit": "mm",
      "model": false
    },
    {
      "name": "h_puck",
      "expr": "4.6",
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
        "cavity"
      ],
      "param_names": [
        "d1",
        "d2",
        "d3"
      ],
      "type": "box",
      "corner": [
        0.0,
        0.0,
        0.0
      ],
      "length": "30",
      "width": "30",
      "height": "20",
      "operation": "new_body"
    },
    {
      "id": 2,
      "name": "Cylinder1",
      "suppressed": false,
      "body_names": [
        "puck"
      ],
      "param_names": [
        "d4",
        "d5"
      ],
      "type": "cylinder",
      "base": [
        15.0,
        15.0,
        "10 - h_puck / 2"
      ],
      "axis": [
        0.0,
        0.0,
        1.0
      ],
      "radius": "r_puck",
      "height": "h_puck",
      "operation": "new_body"
    }
  ],
  "next_id": 3,
  "next_component_color_index": 0
}