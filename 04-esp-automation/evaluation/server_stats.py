import csv
import pycountry
import pycountry_convert as pc

def country_to_continent(country_code):
    try:
        country = pycountry.countries.get(alpha_2=country_code)
        continent_code = pc.country_alpha2_to_continent_code(country.alpha_2)
        return pc.convert_continent_code_to_continent_name(continent_code)
    except:
        return "Unknown"

counts = {
    "Africa": 0,
    "Asia": 0,
    "Europe": 0,
    "North America": 0,
    "South America": 0,
    "Oceania": 0,
}

total = 0

with open("servers_with_coords.csv", encoding="utf-8") as f:
    reader = csv.DictReader(f)
    for row in reader:
        if(row["service"] == "Apple"):
            continent = country_to_continent(row["country"])
            if continent in counts:
                counts[continent] += 1
                total += 1

print(f"total: {total}")
print(f"europe: {counts['Europe']}")
print(f"north america: {counts['North America']}")
print(f"asia: {counts['Asia']}")
print(f"Oceania: {counts['Oceania']}")
print(f"south america: {counts['South America']}")
print(f"africa: {counts['Africa']}")
