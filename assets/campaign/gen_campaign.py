#!/usr/bin/env python3
"""Generates assets/campaign/campaign.json (IGNIS campaign). Run: python3 gen.py <out.json>"""
import json, sys, math

H = 288
FL = H - 7  # first free row above the floor
CY = H // 2

def R(x0, y0, x1, y1, mat=None, tile=None, **kw):
    d = {"op": "rect", "x0": x0, "y0": y0, "x1": x1, "y1": y1}
    if mat is not None: d["mat"] = mat
    if tile is not None: d["tile"] = tile
    d.update(kw); return d
def P(x, y, tile, mat=None):
    d = {"op": "put", "x": x, "y": y, "tile": tile}
    if mat is not None: d["mat"] = mat
    return d
def HOL(x0, y0, x1, y1, t, mat, open_top=False, tile=None):
    d = {"op": "hollow", "x0": x0, "y0": y0, "x1": x1, "y1": y1, "t": t, "mat": mat, "open_top": open_top}
    if tile: d["tile"] = tile
    return d
def MIX(x0, y0, x1, y1, mats): return {"op": "mix", "x0": x0, "y0": y0, "x1": x1, "y1": y1, "mats": mats}
def HEAP(x, base, w, h, mats): return {"op": "heap", "x": x, "base": base, "w": w, "h": h, "mats": mats}
def LINE(x0, y0, x1, y1, w, mat=None, tile=None):
    d = {"op": "line", "x0": x0, "y0": y0, "x1": x1, "y1": y1, "w": w}
    if mat is not None: d["mat"] = mat
    if tile: d["tile"] = tile
    return d
def STR(x0, y0, x1, y1, period, on, mat): return {"op": "stripes", "x0": x0, "y0": y0, "x1": x1, "y1": y1, "period": period, "on": on, "mat": mat}
def GAS(kind, **kw):
    d = {"op": "gas", "kind": kind}; d.update(kw); return d
def SCAT(x0, y0, x1, y1, density, mats, seed=1): return {"op": "scatter", "x0": x0, "y0": y0, "x1": x1, "y1": y1, "density": density, "mats": mats, "seed": seed}
def CLR(x0, y0, x1, y1): return {"op": "clear", "x0": x0, "y0": y0, "x1": x1, "y1": y1}
FLOOR = {"op": "floor"}

def O(text, goal, metric, value, secs=None, star=False):
    d = {"text": text, "goal": goal, "metric": metric, "value": value, "kind": "star" if star else "required"}
    if secs is not None: d["secs"] = secs
    return d

METALS = ["wall:steel", "wall:aluminium", "wall:copper", "wall:titanium", "wall:inconel", "wall:ceramic", "wall:cooled_copper"]

# ---------------------------------------------------------------- shared layouts

def turbojet(hot="inconel"):
    """Scene::turbojet expressed as primitives (hot = material of the hot section + nozzle)."""
    half, top, bot = 19, CY - 19, CY + 19
    x0, ln = 90, 210
    out = []
    # casing: cold steel ends, hot middle
    for (a, b, m) in [(x0, x0 + 19, "steel"), (x0 + 20, x0 + 159, hot), (x0 + 160, x0 + ln, "steel")]:
        out.append(R(a, top - 4, b, top - 1, m))
        out.append(R(a, bot, b, bot + 3, m))
    # converging nozzle then straight throat
    n0, n1 = x0 + 100, x0 + 155
    for x in range(n0, x0 + ln + 1):
        h = half - 11.0 * (x - n0) / (n1 - n0) if x < n1 else half - 11.0
        h = int(math.floor(h + 0.5))
        if top <= CY - h - 1:
            out.append(R(x, top, x, CY - h - 1, hot))
            out.append(R(x, CY + h, x, bot - 1, hot))
    out.append(R(x0 + 4, top, x0 + 13, bot - 1, tile="fan", dir="right"))
    out.append(R(x0 + 40, top - 1, x0 + 51, top - 1, tile="fuel_in"))
    out.append(R(x0 + 40, bot, x0 + 51, bot, tile="fuel_in"))
    out.append(R(x0 + 76, top - 1, x0 + 78, top - 1, tile="igniter"))
    out.append(R(x0 + 76, bot, x0 + 78, bot, tile="igniter"))
    for k in range(8):
        out.append(P(x0 + 66 + k // 2, CY - 3 + k, "wall", hot))
        out.append(P(x0 + 66 + k // 2, CY + 3 - k, "wall", hot))
    return out

def rocket_plate():
    out = [R(55, CY - 30, 59, CY + 29, "steel")]
    for y in range(CY - 22, CY + 22):
        out.append(P(60, y, "fuel_in" if (y - CY) % 4 == 0 else "o2_in"))
    out.append(R(63, CY - 23, 65, CY - 23, tile="igniter"))
    out.append(R(63, CY + 22, 65, CY + 22, tile="igniter"))
    return out

def rocket_body():
    out = []
    xf, xc, xt, xe = 60, 140, 160, 220
    def half(x):
        if x < xc: return 22
        if x < xt:
            t = (x - xc) / (xt - xc); return int(math.floor(22 - 17 * math.sin(t * math.pi / 2) + 0.5))
        t = (x - xt) / (xe - xt); return int(math.floor(5 + 15 * t ** 0.6 + 0.5))
    for x in range(xf, xe + 1):
        h = half(x)
        out.append(R(x, CY - h - 2, x, CY - h - 1, "cooled_copper"))
        out.append(R(x, CY - h - 8, x, CY - h - 3, "steel"))
        out.append(R(x, CY + h, x, CY + h + 1, "cooled_copper"))
        out.append(R(x, CY + h + 2, x, CY + h + 7, "steel"))
    # keep the plate's igniters (the liner rows overlap them)
    out.append(R(63, CY - 23, 65, CY - 23, tile="igniter"))
    out.append(R(63, CY + 22, 65, CY + 22, tile="igniter"))
    return out

def srm_case():
    x0, x1, ch = 70, 190, 26
    return [R(x0, CY - ch - 3, x1, CY - ch, "steel"), R(x0, CY + ch, x1, CY + ch + 3, "steel"),
            R(x0 - 4, CY - ch - 3, x0 - 1, CY + ch + 3, "steel"), R(x0, CY - 7, x0, CY + 7, tile="igniter")]

def srm_solution():
    x0, x1, ch = 70, 190, 26
    out = [R(x0 + 1, CY - ch + 1, x1 - 7, CY - 8, "propellant"), R(x0 + 1, CY + 8, x1 - 7, CY + ch - 1, "propellant")]
    for x in range(x1, x1 + 50):
        h = 25 - (x - x1) if x < x1 + 18 else 7 + int((x - x1 - 18) * 0.45)
        out.append(R(x, CY - h - 5, x, CY - h - 1, "inconel"))
        out.append(R(x, CY + h, x, CY + h + 4, "inconel"))
    return out

def candy_case():
    cx, hw, ih = 256, 16, 70
    bot = FL; top = bot - ih
    out = [FLOOR, R(cx - hw - 4, top - 4, cx - hw - 1, bot, "steel"), R(cx + hw + 1, top - 4, cx + hw + 4, bot, "steel"),
           R(cx - hw - 4, top - 4, cx - 5, top - 1, "steel"), R(cx + 5, top - 4, cx + hw + 4, top - 1, "steel")]
    for k in range(14):
        y = top - 5 - k
        for dx in (-6 - k // 2, -7 - k // 2, 6 + k // 2, 7 + k // 2):
            out.append(P(cx + dx, y, "wall", "steel"))
    return out

def peroxide(with_catalyst):
    cx, half = 256, 22
    out = [HOL(cx - half - 3, 20, cx + half + 3, 60, 3, "steel"), R(cx - half, 23, cx + half, 54, "hydrogen_peroxide"),
           HOL(cx - half - 3, 58, cx + half + 3, 140, 3, "steel", open_top=True), CLR(cx - 1, 58, cx, 60),
           R(cx - 14, 100, cx + 14, 101, "steel"), R(cx - 14, 95, cx - 13, 99, "steel"), R(cx + 13, 95, cx + 14, 99, "steel")]
    # the bottom of the chamber is open into the nozzle
    out.append(CLR(cx - half, 138, cx + half, 140))
    for y in range(138, 190):
        h = half - (y - 138) * 17 // 20 if y < 158 else 5 + (y - 158) // 3
        out.append(R(cx - h - 4, y, cx - h - 1, y, "inconel"))
        out.append(R(cx + h, y, cx + h + 3, y, "inconel"))
    if with_catalyst:
        out.append(R(cx - 12, 97, cx + 12, 99, "manganese_dioxide"))
    return out

# ---------------------------------------------------------------- the campaign

chapters = [
    {"id": 1, "title": "Fire Basics", "blurb": "Fuel, heat and oxygen: take one away and the fire dies. Learn to light, feed and starve a flame."},
    {"id": 2, "title": "Powders & Mixtures", "blurb": "Grains that burn, mixes that carry their own oxygen, and dust that turns a room into a fireball."},
    {"id": 3, "title": "Liquids & Phase Changes", "blurb": "Boiling, freezing, floating fuels and liquid fire. Heat moves matter between solid, liquid and gas."},
    {"id": 4, "title": "Engines & Rockets", "blurb": "Contain the fire, point it somewhere and you have thrust. Build jets, motors and rocket engines."},
    {"id": 5, "title": "Demolition & Extreme Chemistry", "blurb": "Shock waves, containment, runaway reactions. The most violent chemistry in the lab, tamed for the job."},
]

M = []
def mission(**kw):
    kw.setdefault("live", False)
    kw.setdefault("locked_start", True)
    kw.setdefault("gravity", True)
    kw.setdefault("view", 0)
    M.append(kw)

# ===== Chapter 1: Fire Basics
mission(id="first-light", chapter=1, title="First Light", tagline="Light the fuel cloud",
    briefing="A cloud of methane is drifting over the test range. Light it with the Torch: the middle is too rich to burn, so touch the edge where it mixes with air. Then spray oxygen in and make it roar.",
    hints=["Gases view (key 5) shows where the fuel is.", "Methane only burns between 5% and 15% in air: aim for the fringe of the cloud."],
    live=True, duration=30, solve_at=0.5,
    palette=["gas:torch", "gas:o2", "gas:fuel"],
    layout=[FLOOR, GAS("fuel", x=256, y=190, r=40, repeat=3)],
    solution=[GAS("torch", x=256, y=236, r=6, repeat=3)],
    objectives=[O("Set the cloud on fire", "reach", "burn", 0.3), O("Light it within {s} s", "reach_by", "burn", 0.3, 8, True), O("Make it roar: burn {v} kg/s", "reach", "burn", 1.0, star=True)],
    fact="Gas explosions in homes are usually lit by a light switch or a fridge thermostat sparking at the edge of a leak.")

JAR = (206, 150, 306, FL)
mission(id="fire-needs-air", chapter=1, title="Fire Needs Air", tagline="Keep the burner alight",
    requires=["first-light"],
    briefing="A gas burner is sealed inside a steel jar. Light it and it eats the oxygen in minutes, then chokes on its own exhaust. Cut vents so fresh air can get in and hot gas can get out.",
    hints=["Hot exhaust rises: a vent high up lets it out, a vent low down lets fresh air in.", "This is why a candle under a glass goes out, and why chimneys work."],
    duration=60, feeds={"fuel": 1.3},
    palette=["part:vent", "part:air_in", "part:fan", "erase"],
    layout=[FLOOR, HOL(JAR[0], JAR[1], JAR[2], JAR[3], 3, "steel"), R(246, FL - 3, 266, FL, "steel"), R(252, FL - 4, 260, FL - 4, tile="fuel_in"), P(250, FL - 4, "igniter"), P(262, FL - 4, "igniter")],
    solution=[R(236, JAR[1], 276, JAR[1] + 2, tile="vent"), R(JAR[0], FL - 12, JAR[0] + 2, FL - 2, tile="vent"), R(JAR[2] - 2, FL - 12, JAR[2], FL - 2, tile="vent")],
    objectives=[O("Keep the flame burning {v} kg/s for {s} s", "hold", "burn", 0.1, 40), O("Keep it alight for {s} s", "hold", "burn", 0.1, 55, True), O("Burn {v} kg/s", "reach", "burn", 0.25, star=True)],
    fact="Fire fighters fear 'backdraft': a fire starved of air inside a closed room, which explodes when a door lets oxygen back in.")

mission(id="campfire", chapter=1, title="Campfire", tagline="Get a wood fire going",
    requires=["first-light"],
    briefing="Stack logs between the stones and put an igniter in the pile. Hot wood gives off flammable gas, that gas burns and heats more wood. Build a fire that chars away a good amount of wood.",
    hints=["Leave gaps between logs so air can get in.", "Stack them so flames from below heat the wood above."],
    duration=60, budgets=[{"metric": "used:wood", "max": 1200}],
    palette=["wall:wood", "part:igniter", "wall:steel"],
    layout=[FLOOR, R(212, FL - 4, 218, FL, "steel"), R(294, FL - 4, 300, FL, "steel")],
    solution=[STR(233, FL - 3, 278, FL, 9, 7, "wood"), STR(237, FL - 8, 274, FL - 5, 9, 7, "wood"), STR(241, FL - 13, 270, FL - 10, 9, 7, "wood"), STR(245, FL - 18, 266, FL - 15, 9, 7, "wood"),
              P(254, FL - 5, "igniter"), P(258, FL - 5, "igniter")],
    objectives=[O("Burn away {v} cells of wood", "reach", "lost:wood", 25), O("Burn {v} cells within {s} s", "reach_by", "lost:wood", 18, 12, True), O("Use at most {v} wood cells", "at_most", "used:wood", 450, star=True)],
    fact="Wood itself doesn't burn: heat breaks it into flammable gases (pyrolysis), and those gases are the flames you see.")

mission(id="cutting-torch", chapter=1, title="Cutting Torch", tagline="Pure oxygen flame",
    requires=["campfire"],
    briefing="A metal hatch blocks the tunnel. An air flame would take all day, but methane burning in pure oxygen runs a thousand degrees hotter. Build a torch from fuel and oxygen inlets and melt through.",
    hints=["Back the inlets with a wall so the jet points at the plate.", "Mix roughly one fuel inlet for every two or three oxygen inlets."],
    duration=80, feeds={"fuel": 3.0, "o2": 4.0},
    palette=["part:fuel_in", "part:o2_in", "part:igniter", "wall:steel", "wall:ceramic"],
    layout=[FLOOR, R(330, 150, 331, 225, "steel"), R(331, 226, 331, 244, "aluminium"), R(330, 245, 331, FL, "steel")],
    solution=[R(323, 222, 325, 248, "ceramic"), R(326, 223, 326, 247, tile="o2_in"), P(326, 226, "fuel_in"), P(326, 229, "fuel_in"), P(326, 232, "fuel_in"), P(326, 235, "fuel_in"), P(326, 238, "fuel_in"), P(326, 241, "fuel_in"), P(326, 244, "fuel_in"), P(327, 222, "igniter"), P(327, 248, "igniter")],
    objectives=[O("Melt {v} cells of the hatch", "reach", "lost:aluminium", 3), O("Melt through within {s} s", "reach_by", "lost:aluminium", 3, 45, True), O("Melt {v} cells", "reach", "lost:aluminium", 12, star=True)],
    fact="An oxy-fuel torch doesn't just melt steel: once the metal is red hot, the oxygen jet burns the iron itself.")

mission(id="shut-the-valve", chapter=1, title="Shut the Valve", tagline="Starve the fire of fuel",
    requires=["campfire"],
    briefing="A broken gas pipe is burning like a giant torch. Water and sand won't put it out for long, and if they did, the gas would keep pouring out and explode later. Fire crews never put out a gas fire until the supply is shut: cut the fuel.",
    hints=["The green fuel inlet cells at the bottom of the pipe are the leak.", "Erase them and the flame dies on its own."],
    live=True, locked_start=False, duration=75, solve_at=8.0, feeds={"fuel": 2.0},
    palette=["erase", "matter:water", "matter:sand"],
    layout=[FLOOR, R(248, FL - 16, 250, FL, "steel"), R(262, FL - 16, 264, FL, "steel"), R(251, FL, 261, FL, tile="fuel_in"), P(247, FL - 16, "igniter"), P(265, FL - 16, "igniter")],
    solution=[CLR(251, FL, 261, FL)],
    objectives=[O("Put the fire out for {s} s", "hold_below", "burn", 0.03, 3), O("Don't use any water", "at_most", "used:water", 0, star=True), O("Don't use any sand", "at_most", "used:sand", 0, star=True)],
    fact="Burning gas wells have been put out with explosives that blow the flame away from the wellhead, but the well still has to be capped before anyone can relax.")

mission(id="forge", chapter=1, title="Forge", tagline="Heat a steel bar",
    requires=["cutting-torch"],
    briefing="A blacksmith needs this heavy steel bar heated through. Steel soaks up a lot of heat before its temperature climbs, so build a strong burner under it and feed it air. The more enclosed the fire, the faster the bar heats.",
    hints=["Ceramic walls insulate: a little oven around the bar keeps the heat in.", "Air inlets feed oxygen to the flame; too much air cools it."],
    duration=50, feeds={"fuel": 2.0, "air": 2.0},
    palette=["part:fuel_in", "part:air_in", "part:igniter", "part:fan", "wall:ceramic", "wall:steel"],
    layout=[FLOOR, R(226, 200, 286, 203, "steel"), R(222, 200, 225, FL, "ceramic"), R(287, 200, 290, FL, "ceramic")],
    solution=[R(236, FL - 6, 276, FL - 6, tile="fuel_in"), R(236, FL - 5, 276, FL - 4, tile="air_in"), P(234, FL - 7, "igniter"), P(278, FL - 7, "igniter"), R(222, 190, 290, 192, "ceramic"), R(222, 193, 225, 199, "ceramic"), R(287, 193, 290, 199, "ceramic")],
    objectives=[O("Heat the bar to {v}% of its melting point", "reach", "wall_heat", 29), O("Do it within {s} s", "reach_by", "wall_heat", 29, 32, True), O("Reach {v}%", "reach", "wall_heat", 32, star=True)],
    fact="Steel glows dull red at about 700 °C and turns yellow-white near 1200 °C; smiths judged temperature by colour for 3000 years.")

# ===== Chapter 2: Powders & Mixtures
mission(id="charcoal-grill", chapter=2, title="Charcoal Grill", tagline="Glowing embers",
    requires=["campfire"],
    briefing="Charcoal is wood with everything but the carbon cooked off. It barely flames, it glows. Pile it in the grill tray, light it and get the embers hot enough to sear.",
    hints=["A taller heap keeps the heat in the middle.", "Charcoal burns at its surface: it needs air touching it."],
    duration=40, igniter_secs=2, budgets=[{"metric": "used:charcoal", "max": 1500}],
    palette=["matter:charcoal", "part:igniter", "part:fan", "matter:sand"],
    layout=[FLOOR, HOL(200, FL - 22, 312, FL, 3, "steel", open_top=True)],
    solution=[HEAP(256, FL - 3, 40, 14, ["charcoal"]), P(256, FL - 17, "igniter"), P(236, FL - 9, "igniter"), P(276, FL - 9, "igniter")],
    objectives=[O("Keep the embers above {v} K for {s} s", "hold", "peak_temp", 1000, 10), O("Keep them above {v} K for {s} s", "hold", "peak_temp", 1000, 25, True), O("Use at most {v} charcoal", "at_most", "used:charcoal", 650, star=True)],
    fact="Charcoal burns at about 1100 °C with almost no smoke, which is why it was the fuel of bronze and iron smelting.")

mission(id="magnesium-flare", chapter=2, title="Magnesium Flare", tagline="The brightest fire",
    requires=["charcoal-grill"],
    briefing="Magnesium is a metal that burns: once it catches it blazes with a white light too bright to look at. Emergency flares and old camera flashes use it. Light a pile and keep it blazing.",
    hints=["The igniter only stays on for a moment: it has to touch the metal.", "A bigger pile burns longer."],
    duration=40, igniter_secs=1.5, budgets=[{"metric": "used:magnesium", "max": 1200}],
    palette=["matter:magnesium", "part:igniter", "matter:sand", "wall:ceramic"],
    layout=[FLOOR, HOL(220, FL - 20, 292, FL, 3, "ceramic", open_top=True)],
    solution=[HEAP(256, FL - 3, 30, 12, ["magnesium"]), P(256, FL - 16, "igniter"), P(234, FL - 6, "igniter"), P(278, FL - 6, "igniter")],
    objectives=[O("Hold {v} K for {s} s", "hold", "peak_temp", 1300, 5), O("Hold {v} K for {s} s", "hold", "peak_temp", 1300, 15, True), O("Use at most {v} magnesium", "at_most", "used:magnesium", 500, star=True)],
    fact="Magnesium fires can't be put out with water: the hot metal tears the water apart and burns the hydrogen too.")

mission(id="rocket-candy", chapter=2, title="Rocket Candy", tagline="Fuel a sugar rocket",
    requires=["charcoal-grill"],
    briefing="This steel motor case stands nozzle-up on a test stand. Fill it with a mix of saltpeter and sugar: saltpeter releases oxygen when hot, so the mix burns by itself and roars out of the nozzle.",
    hints=["Mix the two powders well: alternate strokes or use Spray.", "Leave room above the grain for gas to build up."],
    duration=30,
    palette=["matter:saltpeter", "matter:sugar", "part:igniter", "wall:steel"],
    layout=candy_case(),
    solution=[MIX(240, FL - 54, 272, FL, ["saltpeter", "sugar"]), R(255, FL - 55, 256, FL - 55, tile="igniter")],
    objectives=[O("Make {v} kN of thrust", "reach", "thrust", 15), O("Hold {v} kN for {s} s", "hold", "thrust", 10, 3, True), O("The case survives", "keep", "struct_lost", 0.5, star=True)],
    fact="Amateur rocketeers have flown sugar-and-saltpeter motors to over 10 km altitude.")

mission(id="rail-weld", chapter=2, title="Rail Weld", tagline="Thermite through steel",
    requires=["rocket-candy"],
    briefing="Rust and aluminium powder look harmless, but once lit the aluminium tears the oxygen out of the rust and leaves white-hot molten iron. Railway crews weld track this way. Burn through the steel plate.",
    hints=["Pile the mix right on the plate.", "It is hard to light: the igniter must touch the mix."],
    duration=40, budgets=[{"metric": "used:al_powder", "max": 800}],
    palette=["matter:rust", "matter:al_powder", "part:igniter", "matter:sand", "matter:magnesium"],
    layout=[FLOOR, R(196, 160, 316, 161, "steel"), R(196, 162, 201, FL, "steel"), R(311, 162, 316, FL, "steel"), R(196, 154, 198, 159, "steel"), R(314, 154, 316, 159, "steel")],
    solution=[MIX(236, 146, 275, 159, ["al_powder", "rust"]), P(256, 145, "igniter")],
    objectives=[O("Melt {v} cells of steel", "reach", "lost:steel", 8), O("Burn through within {s} s", "reach_by", "lost:steel", 8, 12, True), O("Use at most {v} aluminium powder", "at_most", "used:al_powder", 400, star=True)],
    fact="Thermite reaches about 2500 °C. Rail welders pour it into a mould around the gap between two rails.")

mission(id="signal-fire", chapter=2, title="Signal Fire", tagline="Lay a fuse",
    requires=["rocket-candy"],
    briefing="The igniter is in a bunker; the signal brazier is on the far hill. Run a fuse from one to the other, over the rocks and into the magnesium kindling. A fuse is a thread of slow-burning mix that carries a flame exactly where you lay it.",
    hints=["The fuse must touch the igniter at one end and the magnesium kindling at the other.", "Fuses burn round corners, but keep the line unbroken."],
    duration=60, budgets=[{"metric": "used:fuse", "max": 1000}],
    palette=["matter:fuse", "matter:magnesium", "erase"],
    layout=[FLOOR, R(20, FL - 30, 22, FL, "steel"), R(20, FL - 30, 60, FL - 28, "steel"), R(58, FL - 27, 60, FL - 4, "steel"), P(50, FL, "igniter"),
            R(200, FL - 60, 220, FL, "steel"), R(300, 160, 330, FL, "steel"),
            HOL(430, FL - 20, 470, FL, 3, "steel", open_top=True), R(433, FL - 6, 467, FL - 3, "magnesium"), STR(434, FL - 16, 466, FL - 7, 6, 4, "wood")],
    solution=[LINE(51, FL, 199, FL, 0, "fuse"), LINE(199, FL, 199, FL - 61, 0, "fuse"), LINE(199, FL - 61, 221, FL - 61, 0, "fuse"), LINE(221, FL - 61, 221, FL, 0, "fuse"),
              LINE(221, FL, 299, FL, 0, "fuse"), LINE(299, FL, 299, 159, 0, "fuse"), LINE(299, 159, 331, 159, 0, "fuse"), LINE(331, 159, 331, FL, 0, "fuse"),
              LINE(331, FL, 429, FL, 0, "fuse"), LINE(429, FL, 429, FL - 21, 0, "fuse"), LINE(429, FL - 21, 438, FL - 21, 0, "fuse"), LINE(438, FL - 20, 438, FL - 7, 0, "fuse")],
    objectives=[O("Light the brazier (burn {v} wood cells)", "reach", "lost:wood", 5), O("Light it within {s} s", "reach_by", "lost:wood", 5, 40, True), O("Use at most {v} fuse cells", "at_most", "used:fuse", 800, star=True)],
    fact="Safety fuse, invented by William Bickford in 1831 for Cornish tin mines, burns at a steady 1 cm per second.")

MILL = (156, 356, 128)
mission(id="dust-explosion", chapter=2, title="Dust Explosion", tagline="Why flour mills explode",
    requires=["signal-fire"],
    briefing="A heap of flour barely burns: air only reaches its surface. Blow the same flour into a cloud and every grain is wrapped in air, and the whole cloud burns at once. Make the old mill blow its tin roof off.",
    hints=["Spray flour into the air inside the mill, then put an igniter in the cloud.", "Fans blowing up under the heap raise a dust cloud too."],
    duration=25, feeds={"fan": 1.5},
    palette=["matter:flour", "part:igniter", "part:fan", "matter:sugar"],
    layout=[FLOOR, R(MILL[0], MILL[2], MILL[0] + 3, FL, "steel"), R(MILL[1] - 3, MILL[2], MILL[1], FL, "steel"), R(MILL[0], MILL[2], MILL[1], MILL[2] + 1, "aluminium"), HEAP(256, FL, 40, 14, ["flour"])],
    solution=[SCAT(MILL[0] + 8, MILL[2] + 10, MILL[1] - 9, MILL[2] + 89, 0.16, ["flour"], 7), P(256, MILL[2] + 60, "igniter")],
    objectives=[O("Blow out {v} cells of the roof", "reach", "lost:aluminium", 20), O("Do it within {s} s", "reach_by", "lost:aluminium", 20, 15, True), O("Spray at most {v} flour", "at_most", "used:flour", 2500, star=True)],
    fact="Grain dust explosions still happen: modern mills and silos use dust extraction and spark-proof motors to prevent them.")

# ===== Chapter 3: Liquids & Phase Changes
mission(id="alcohol-stove", chapter=3, title="Alcohol Stove", tagline="A clean blue flame",
    requires=["charcoal-grill"],
    briefing="Ethanol evaporates even at room temperature, and it's the vapour, not the liquid, that burns. Pour fuel into the stove cup and light it. Keep a steady flame going for the hikers' tea.",
    hints=["Put the igniter just above the liquid surface, in the vapour.", "A wider pool makes more vapour and a bigger flame."],
    duration=40, budgets=[{"metric": "used:ethanol", "max": 1200}],
    palette=["matter:ethanol", "part:igniter", "wall:steel"],
    layout=[FLOOR, HOL(216, FL - 18, 296, FL, 3, "steel", open_top=True)],
    solution=[R(219, FL - 10, 293, FL - 3, "ethanol"), P(256, FL - 12, "igniter"), P(230, FL - 12, "igniter"), P(282, FL - 12, "igniter")],
    objectives=[O("Hold a {v} kg/s flame for {s} s", "hold", "burn", 0.05, 10), O("Hold it for {s} s", "hold", "burn", 0.05, 25, True), O("Use at most {v} ethanol", "at_most", "used:ethanol", 400, star=True)],
    fact="The V-2 rocket burned a 75% ethanol mix with liquid oxygen; the water in it kept the engine cooler.")

mission(id="fire-brigade", chapter=3, title="Pool Fire", tagline="Put out a kerosene fire",
    requires=["alcohol-stove"],
    briefing="A tray of kerosene is on fire. Water sinks straight under kerosene and so does sand, so neither smothers it: the burning fuel just floats on top. Cut off the air instead and starve the fire.",
    hints=["A steel cover resting on the tray walls seals it.", "Try water first if you like: watch where it goes."],
    live=True, locked_start=False, duration=60, solve_at=6.0, igniter_secs=4,
    palette=["wall:steel", "matter:water", "matter:sand"],
    layout=[FLOOR, R(186, FL - 19, 189, FL, "steel"), R(323, FL - 19, 326, FL, "steel"), R(190, FL - 5, 322, FL, "kerosene")] + [P(x, FL - 6, "igniter") for x in range(196, 316, 16)],
    solution=[R(186, FL - 21, 326, FL - 20, "steel")],
    objectives=[O("Put the fire out for {s} s", "hold_below", "burn", 0.1, 3), O("Don't use any water", "at_most", "used:water", 0, star=True), O("Use at most {v} steel", "at_most", "used:steel", 300, star=True)],
    fact="Foam is the fire brigade's answer to fuel fires: it floats on the fuel like a blanket and cuts off the air.")

mission(id="grease-fire", chapter=3, title="Grease Fire", tagline="Never use water",
    requires=["fire-brigade"],
    briefing="A pan of cooking oil has caught fire. Throw water on it and the water sinks, flashes to steam and blasts burning oil across the kitchen. Smother it instead: a lid or a blanket of sand.",
    hints=["A steel lid over the pan cuts off the air.", "Try the water if you want to see why not. Then reset."],
    live=True, locked_start=False, duration=60, solve_at=5.0, igniter_secs=4,
    palette=["wall:steel", "matter:sand", "matter:water"],
    layout=[FLOOR, R(190, FL - 40, 310, FL, "steel"), R(222, FL - 52, 278, FL - 51, "steel"), R(222, FL - 59, 223, FL - 53, "steel"), R(277, FL - 59, 278, FL - 53, "steel"),
            R(222, FL - 50, 224, FL - 41, "steel"), R(276, FL - 50, 278, FL - 41, "steel"), R(224, FL - 56, 276, FL - 53, "cooking_oil"),
            P(232, FL - 57, "igniter"), P(256, FL - 57, "igniter"), P(268, FL - 57, "igniter")],
    solution=[R(221, FL - 62, 279, FL - 60, "steel")],
    objectives=[O("Put the fire out for {s} s", "hold_below", "burn", 0.1, 3), O("Don't use any water", "at_most", "used:water", 0, star=True), O("Use at most {v} steel", "at_most", "used:steel", 200, star=True)],
    fact="A cup of water on a burning chip pan can throw a fireball several metres across a kitchen.")

mission(id="ice-dam", chapter=3, title="Ice Dam", tagline="Melt the ice",
    requires=["alcohol-stove"],
    briefing="A block of ice has jammed the spillway. Melting ice soaks up a surprising amount of heat before it even gets warmer. Build a burner under the ice and turn it into running water.",
    hints=["The flame has to touch the ice; hot gas rises, so heat it from below.", "Copper spreads heat into the block fast."],
    duration=60, feeds={"fuel": 1.6},
    palette=["part:fuel_in", "part:igniter", "part:air_in", "wall:copper", "wall:steel"],
    layout=[FLOOR, R(180, 200, 186, FL, "steel"), R(326, 200, 332, FL, "steel"), R(187, 200, 325, 201, "steel"), R(200, 150, 312, 199, "ice")],
    solution=[R(200, FL - 2, 312, FL - 2, tile="fuel_in"), P(198, FL - 3, "igniter"), P(314, FL - 3, "igniter"), R(220, FL - 30, 222, FL - 30, tile="igniter"), R(290, FL - 30, 292, FL - 30, tile="igniter")],
    objectives=[O("Melt {v} cells into water", "reach", "count:water", 100), O("Within {s} s", "reach_by", "count:water", 100, 40, True), O("Melt {v} cells", "reach", "count:water", 200, star=True)],
    fact="Melting 1 kg of ice takes as much heat as warming that water from 0 to 80 °C.")

GAR = (150, 180, 360)
mission(id="fuel-vapour", chapter=3, title="Fuel Vapour", tagline="Fumes travel",
    requires=["ice-dam"],
    briefing="Gasoline evaporates fast, and its vapour creeps along the floor until it finds a flame. That's how a spill on one side of a garage meets a water-heater pilot light on the other. Spill some fuel well away from the flame and watch the vapour find it.",
    hints=["You don't have to spill it near the pilot light: the vapour spreads.", "More spilled fuel means more vapour, sooner."],
    duration=40,
    palette=["matter:gasoline", "matter:diesel", "matter:water"],
    layout=[FLOOR, HOL(GAR[0], GAR[1], GAR[2], FL, 3, "steel"), R(GAR[2] - 2, 220, GAR[2], FL - 3, "aluminium"), P(GAR[0] + 6, FL - 3, "igniter")],
    solution=[R(200, FL - 5, 290, FL - 3, "gasoline")],
    objectives=[O("The vapour reaches the flame (burn {v} kg/s)", "reach", "burn", 0.1), O("Within {s} s", "reach_by", "burn", 0.1, 25, True), O("Spill at most {v} gasoline", "at_most", "used:gasoline", 400, star=True)],
    fact="Gasoline vapour is about three times heavier than air, so it pools along floors and flows down into basements and drains.")

mission(id="foundry-pour", chapter=3, title="Foundry Pour", tagline="Make liquid iron",
    requires=["fuel-vapour"],
    briefing="An old foundry trick: thermite doesn't just burn, it leaves a pool of white-hot liquid iron behind. Fill the crucible with the mix, light it and make a big enough pour to cast with.",
    hints=["Rust plus aluminium powder, mixed well.", "The igniter has to touch the mix."],
    duration=30, budgets=[{"metric": "used:al_powder", "max": 1000}],
    palette=["matter:rust", "matter:al_powder", "part:igniter", "matter:sand"],
    layout=[FLOOR, HOL(216, FL - 40, 296, FL, 3, "ceramic", open_top=True), R(219, FL - 6, 293, FL - 3, "sand")],
    solution=[MIX(222, FL - 22, 290, FL - 7, ["al_powder", "rust", "rust"]), P(256, FL - 23, "igniter")],
    objectives=[O("Make {v} cells of liquid iron", "reach", "count:molten_metal", 100), O("Within {s} s", "reach_by", "count:molten_metal", 100, 5, True), O("Make {v} cells", "reach", "count:molten_metal", 400, star=True)],
    fact="Thermite was invented in 1893 by Hans Goldschmidt, who was looking for a way to make very pure metals without carbon.")

# ===== Chapter 4: Engines & Rockets
mission(id="hot-section", chapter=4, title="Hot Section", tagline="Fix the turbojet",
    requires=["forge"],
    briefing="This turbojet's hot section was built from aluminium, and it melts the moment the engine lights. Rebuild it from something that can take the heat, then make 40 kN without any wall going over 80% of its melting point.",
    hints=["Hover a material to compare melting points.", "More fuel means more thrust but hotter walls."],
    duration=30, locked_start=False, feeds={"fuel": 3.0, "air": 2.0, "fan": 1.0, "adjustable": True},
    palette=["wall:steel", "wall:aluminium", "wall:titanium", "wall:inconel", "wall:ceramic", "wall:cooled_copper", "part:fan", "part:fuel_in", "part:igniter"],
    layout=turbojet("aluminium"), solution=turbojet("inconel"),
    objectives=[O("Hold {v} kN for {s} s", "hold", "thrust", 40, 3), O("No wall above {v}% of melting", "keep", "wall_heat", 80), O("Hold {v} kN for {s} s", "hold", "thrust", 60, 3, True), O("No wall lost", "keep", "struct_lost", 0.5, star=True)],
    fact="Turbine blades run hotter than the melting point of their own alloy: cooling air flows out of tiny holes and wraps them in a film.")

mission(id="solid-motor", chapter=4, title="Solid Motor", tagline="Cast a rocket grain",
    requires=["rocket-candy"],
    briefing="Here's an empty steel motor case with an igniter at the head end. Cast a propellant grain inside and build a nozzle. Solid propellant burns at its exposed surface, faster at higher pressure: the grain's shape is the throttle.",
    hints=["Leave a hollow bore down the middle so the whole length burns at once.", "A throat that's too narrow raises pressure until the case bursts."],
    duration=25, gravity=False, budgets=[{"metric": "used:propellant", "max": 6000}],
    palette=["wall:propellant", "wall:steel", "wall:titanium", "wall:inconel", "wall:ceramic"],
    layout=srm_case(), solution=srm_solution(),
    objectives=[O("Hold {v} kN for {s} s", "hold", "thrust", 120, 2), O("The case survives (no wall lost)", "keep", "struct_lost", 0.5, star=True), O("Total impulse {v} kN·s", "reach", "impulse", 1500, star=True)],
    fact="Each Space Shuttle booster burned 500 tonnes of propellant in two minutes; the star-shaped bore set its thrust curve.")

mission(id="liftoff", chapter=4, title="Liftoff", tagline="Build a rocket engine",
    requires=["hot-section"],
    briefing="You have an injector plate spraying methane and oxygen at 12 atm. Build the chamber and nozzle around it. The engine must make 150 kN for 10 seconds without falling apart.",
    hints=["A chamber, a narrow throat, then a widening bell.", "Rocket flames are 3500 K: line the hot side with cooled copper and back it with steel."],
    duration=25, gravity=False, feeds={"fuel": 12.0, "o2": 12.0},
    palette=METALS,
    layout=rocket_plate(), solution=rocket_body(),
    objectives=[O("Hold {v} kN for {s} s", "hold", "thrust", 150, 10), O("Lose at most {v} wall cells", "keep", "struct_lost", 30), O("Hold {v} kN for {s} s", "hold", "thrust", 200, 5, True), O("Use at most {v} wall cells", "at_most", "used_walls", 2600, star=True)],
    fact="SpaceX's Raptor runs its chamber at about 300 atm, the highest of any engine ever flown.")

mission(id="peroxide-thruster", chapter=4, title="Peroxide Thruster", tagline="A catalyst does the work",
    requires=["liftoff"],
    briefing="Strong hydrogen peroxide is stable on its own, but drip it onto a catalyst and it splits into hot steam and oxygen. No spark, no fuel, just thrust. Load the catalyst bed and fire the thruster.",
    hints=["The catalyst goes on the tray under the drip hole.", "It is not used up, so a thin bed is enough."],
    duration=30, gravity=True, budgets=[{"metric": "used:manganese_dioxide", "max": 400}],
    palette=["matter:manganese_dioxide", "matter:sand", "wall:steel"],
    layout=peroxide(False), solution=[R(244, 97, 268, 99, "manganese_dioxide")],
    objectives=[O("Make {v} kN of thrust", "reach", "thrust", 40), O("Hold {v} kN for {s} s", "hold", "thrust", 30, 5, True), O("Use at most {v} catalyst", "at_most", "used:manganese_dioxide", 100, star=True)],
    fact="Peroxide thrusters steered the X-15 rocket plane and the Mercury capsules, and powered the famous rocket belts.")

mission(id="hybrid-motor", chapter=4, title="Hybrid Motor", tagline="Burn a wooden rocket",
    requires=["solid-motor"],
    briefing="A hybrid rocket keeps the fuel solid and the oxidiser as a gas. Here the oxygen feed is ready; line the case with a wooden fuel grain. Oxygen alone just blows out cold. Burning fuel makes hot, fast gas and real thrust.",
    hints=["The wood only burns where the oxygen flow touches it.", "Leave a channel down the middle for the oxygen to flow through."],
    duration=30, gravity=False, feeds={"o2": 4.0},
    palette=["wall:steel", "wall:inconel", "wall:ceramic", "wall:wood", "part:o2_in"],
    layout=[R(60, CY - 30, 63, CY + 30, "steel"), R(64, CY - 30, 200, CY - 27, "steel"), R(64, CY + 27, 200, CY + 30, "steel"),
            R(64, CY - 8, 64, CY + 8, tile="o2_in"), P(66, CY - 10, "igniter"), P(66, CY + 10, "igniter")],
    solution=[R(70, CY - 26, 196, CY - 14, "wood"), R(70, CY + 14, 196, CY + 26, "wood")],
    objectives=[O("Burn {v} kg/s of fuel", "reach", "burn", 1.0), O("Hold {v} kN for {s} s", "hold", "thrust", 40, 3), O("Hold {v} kN for {s} s", "hold", "thrust", 40, 10, True), O("Lose no structural wall", "keep", "struct_lost", 0.5, star=True)],
    fact="SpaceShipOne, the first private craft to reach space (2004), flew on a hybrid motor burning rubber with nitrous oxide.")

# ===== Chapter 5: Demolition & Extreme Chemistry
VX0, VX1, VY0 = 300, 420, H - 110
mission(id="blast-door", chapter=5, title="Blast Door", tagline="Breach the titanium vault",
    requires=["rail-weld"],
    briefing="The vault is solid titanium, the strongest wall there is. Place a demolition charge against it and an igniter to set it off. Charges are expensive: the less you use, the more stars.",
    hints=["Blasts push in every direction.", "A charge pressed flat against the wall, or tamped behind a steel block, puts more of its energy into the vault."],
    duration=8, budgets=[{"metric": "used:explosive", "max": 400}],
    palette=["wall:explosive", "part:igniter", "wall:steel", "matter:sand"],
    layout=[FLOOR, R(VX0, VY0, VX0 + 2, FL, "titanium"), R(VX1 - 2, VY0, VX1, FL, "titanium"), R(VX0, VY0, VX1, VY0 + 2, "titanium")],
    solution=[R(VX0 - 8, H - 30, VX0 - 1, FL, "explosive"), P(VX0 - 9, H - 10, "igniter")],
    objectives=[O("Blow out {v} cells of the vault", "reach", "lost:titanium", 30), O("Use at most {v} charge cells", "at_most", "used:explosive", 130, star=True), O("Use at most {v} charge cells", "at_most", "used:explosive", 80, star=True)],
    fact="A detonation front moves faster than sound in the charge itself, several km/s, so the solid becomes gas before it can move.")

mission(id="containment", chapter=5, title="Containment", tagline="Survive the blast",
    requires=["blast-door"],
    briefing="There's a test charge in the middle of the range. Build a sealed vessel around it that survives the blast without springing a leak. Lighter vessels earn more stars.",
    hints=["Give the blast room: pressure drops as the gas spreads into a bigger volume.", "Titanium is strongest per cell."],
    duration=10, gravity=False, view=1,
    palette=METALS,
    layout=[R(253, 141, 259, 147, "explosive"), P(252, 144, "igniter")],
    solution=[HOL(226, 114, 286, 174, 3, "titanium")],
    objectives=[O("The vessel stays sealed", "keep", "leak", 0.5), O("Use at most {v} wall cells", "at_most", "used_walls", 1000, star=True), O("Use at most {v} wall cells", "at_most", "used_walls", 650, star=True)],
    fact="Bomb-disposal containment vessels are thick steel spheres: a sphere spreads the load evenly, with no corners to crack.")

mission(id="tower-drop", chapter=5, title="Tower Drop", tagline="Bring down the chimney",
    requires=["containment"],
    briefing="An old steel chimney has to come down without wrecking the neighbours. Demolition crews cut the base so the structure fails right where they want it. Knock out the chimney's base.",
    hints=["Charges at the base, against both walls, do the most damage.", "The steel blast shields protect the houses; don't remove them."],
    duration=10, budgets=[{"metric": "used:explosive", "max": 300}],
    palette=["wall:explosive", "part:igniter", "matter:sand", "wall:steel"],
    layout=[FLOOR, HOL(236, 90, 276, FL, 4, "steel", open_top=True), R(150, FL - 50, 153, FL, "steel"), R(359, FL - 50, 362, FL, "steel"),
            HOL(60, FL - 30, 110, FL, 2, "wood"), HOL(400, FL - 30, 450, FL, 2, "wood")],
    solution=[R(240, FL - 6, 272, FL, "explosive"), P(256, FL - 7, "igniter")],
    objectives=[O("Destroy {v} cells of the chimney", "reach", "lost:steel", 60), O("The houses are untouched", "keep", "lost:wood", 0.5, star=True), O("Use at most {v} charge cells", "at_most", "used:explosive", 120, star=True)],
    fact="Controlled demolitions take weeks of planning for a few seconds of action: engineers pre-weaken columns so the building falls into its own footprint.")

mission(id="alkali-splash", chapter=5, title="Alkali Splash", tagline="Metals that burn in water",
    requires=["foundry-pour"],
    briefing="Sodium and potassium are metals so reactive they rip water apart, freeing hydrogen and enough heat to light it. Drop lumps into the tank and make the hottest flare you can.",
    hints=["Potassium is the more violent of the two.", "Drop the lumps from a height so they land in the water."],
    duration=30,
    palette=["matter:sodium", "matter:potassium", "matter:water"],
    layout=[FLOOR, HOL(110, FL - 70, 400, FL, 3, "glass", open_top=True, tile="matter"), R(113, FL - 45, 397, FL - 3, "water")],
    solution=[R(150, 40, 153, 43, "potassium"), R(210, 52, 213, 55, "potassium"), R(270, 40, 273, 43, "potassium"), R(330, 52, 333, 55, "sodium")],
    objectives=[O("Reach {v} K", "reach", "peak_temp", 1200), O("Reach {v} K", "reach", "peak_temp", 1800, star=True), O("Reach {v} atm", "reach", "peak_pressure", 1.3, star=True)],
    fact="Potassium burns with a lilac flame; chemists identify it by that colour in the flame test.")

mission(id="no-matches", chapter=5, title="No Matches", tagline="Fire from a reaction",
    requires=["alkali-splash"],
    briefing="No igniters, no torch: start a fire anyway. Some chemicals react so vigorously on contact that they heat themselves until they burst into flame. Find the pair and let chemistry strike the match.",
    hints=["Potassium permanganate is a strong oxidiser; glycerin is a sweet, syrupy fuel.", "Drop one onto the other in the dish."],
    duration=40,
    palette=["matter:potassium_permanganate", "matter:glycerin", "matter:sand", "matter:water"],
    layout=[FLOOR, HOL(216, FL - 12, 296, FL, 2, "steel", open_top=True)],
    solution=[R(240, FL - 8, 272, FL - 2, "potassium_permanganate"), R(248, FL - 30, 264, FL - 26, "glycerin")],
    objectives=[O("Get the reaction above {v} K", "reach", "peak_temp", 1200), O("Within {s} s", "reach_by", "peak_temp", 1200, 10, True), O("Keep it above {v} K for {s} s", "hold", "peak_temp", 1000, 12, True)],
    fact="Permanganate and glycerin is a classic survival-kit fire starter and a favourite chemistry-lecture demo.")

doc = {"version": 1, "name": "IGNIS campaign", "chapters": chapters,
       "defaults": {"gravity": True, "locked_start": True, "view": 0}, "missions": M}

def dump(o, ind=0):
    """Compact-ish pretty printer: primitives and objectives on one line each."""
    sp = "  " * ind
    if isinstance(o, dict):
        if "op" in o or "goal" in o or ("id" in o and "blurb" in o) or all(not isinstance(v, (dict, list)) for v in o.values()) and len(o) <= 6:
            return json.dumps(o, ensure_ascii=False)
        items = [f'{sp}  {json.dumps(k)}: {dump(v, ind + 1)}' for k, v in o.items()]
        return "{\n" + ",\n".join(items) + f"\n{sp}}}"
    if isinstance(o, list):
        if all(isinstance(v, str) for v in o) and len(json.dumps(o)) < 160:
            return json.dumps(o, ensure_ascii=False)
        if not o: return "[]"
        items = [f"{sp}  {dump(v, ind + 1)}" for v in o]
        return "[\n" + ",\n".join(items) + f"\n{sp}]"
    return json.dumps(o, ensure_ascii=False)

open(sys.argv[1], "w").write("// IGNIS campaign (generated by gen_campaign.py; hand edits are fine too). JSON-lite: // comments allowed.\n" + dump(doc) + "\n")
print(len(M), "missions")
