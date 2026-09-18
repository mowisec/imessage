#!/usr/bin/env python3

import os
import pandas as pd
import matplotlib.pyplot as plt
import cartopy.crs as ccrs
from matplotlib.patches import Patch
from matplotlib.lines import Line2D

from geopy.geocoders import Nominatim
from geopy.extra.rate_limiter import RateLimiter
from matplotlib.font_manager import FontProperties
# -------------------------
# Configuration
# -------------------------

INPUT_CSV = "servers.csv"
CACHED_CSV = "servers_with_coords.csv"
OUTPUT_PDF = "server_locations_map.pdf"

# Cropping (your exact values)
MAP_EXTENT = [-160, 160, -60, 75]

# Marker appearance
MARKER_SIZE = 110
MARKER_EDGE_COLOR = "#218AFF"
MARKER_FACE_COLOR = (1, 1, 1, 0.35)  # transparent white fill
MARKER_EDGE_WIDTH = 2.5

MARKER_COLOR_AMAZON = "#FF9900"     # Amazon

# -------------------------
# Step 1: Load or geocode
# -------------------------

if os.path.exists(CACHED_CSV):
    print(f"Loading cached coordinates from {CACHED_CSV}")
    df = pd.read_csv(CACHED_CSV)
else:
    print("Geocoding server locations (this runs only once)...")

    df = pd.read_csv(INPUT_CSV)

    geolocator = Nominatim(user_agent="server-location-mapper")
    geocode = RateLimiter(geolocator.geocode, min_delay_seconds=1)

    def geocode_row(row):
        query = f"{row['city']}, {row['country']}"
        location = geocode(query)
        if location is None:
            print(f"WARNING: Could not geocode {query}")
            return pd.Series([None, None])
        return pd.Series([location.latitude, location.longitude])

    df[["lat", "lon"]] = df.apply(geocode_row, axis=1)

    # Drop failed lookups
    df = df.dropna(subset=["lat", "lon"])

    df.to_csv(CACHED_CSV, index=False)
    print(f"Saved cached coordinates to {CACHED_CSV}")

# -------------------------
# Step 2: Plot map
# -------------------------

fig = plt.figure(figsize=(15, 6))
ax = plt.axes(projection=ccrs.PlateCarree())

# Coastlines only (paper style)
ax.coastlines(linewidth=3)

# Crop unused regions
ax.set_extent(MAP_EXTENT, crs=ccrs.PlateCarree())

# Remove frame and ticks
ax.set_frame_on(False)

# -------------------------
# Step 3: Plot servers
# -------------------------

# Split by service
df_amazon = df[df["service"] == "Amazon"]
df_apple = df[df["service"] == "Apple"]

# 1) Amazon servers first (background)
ax.scatter(
    df_amazon["lon"],
    df_amazon["lat"],
    s=MARKER_SIZE,
    color=MARKER_COLOR_AMAZON,
    marker="o",
    transform=ccrs.PlateCarree(),
    zorder=5,
)

# 2) Apple servers second (foreground)
ax.scatter(
    df_apple["lon"],
    df_apple["lat"],
    s=MARKER_SIZE,
    marker="o",
    facecolors=MARKER_FACE_COLOR,
    edgecolors=MARKER_EDGE_COLOR,
    linewidths=MARKER_EDGE_WIDTH,
    transform=ccrs.PlateCarree(),
    zorder=6,
)


legend_elements = [
    Line2D([0], [0], marker='o', color='w', markerfacecolor=MARKER_COLOR_AMAZON,
           markersize=10, label='AWS EC2 Server'),
    Line2D([0], [0], marker='o', color='w', markerfacecolor=MARKER_FACE_COLOR,
           markeredgecolor=MARKER_EDGE_COLOR, markeredgewidth=2, markersize=10, label='iMessage Server')
]

legend_font = FontProperties(
    #family='Liberation Serif',        # or 'sans-serif', 'monospace'
    size=18,
    weight='normal'        # 'bold', 'light'
)

ax.legend(
    handles=legend_elements,
    loc='lower left',
    frameon=True,
    prop=legend_font
)
# -------------------------
# Step 4: Save figure
# -------------------------

plt.tight_layout()
plt.savefig(OUTPUT_PDF, bbox_inches="tight", dpi=300)
# plt.show()

print(f"Map saved as {OUTPUT_PDF}")
