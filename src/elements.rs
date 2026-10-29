//! The element catalogue shown in the Powder-Toy-style browser: materials, machines, gas
//! brushes and tools, with game descriptions and real-world facts for the tooltips.
#![allow(dead_code)]
use crate::scene;
use eframe::egui::Color32;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Wall(u32),
    Part(u32),
    Gas(u32),
    /// powder / liquid matter (tile 8), element id
    Matter(u32),
    Erase,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cat {
    Structure,
    Burnable,
    Powders,
    Liquids,
    Machines,
    Gases,
    Tools,
    /// reactive metals and metal powders
    Metals,
    /// acids, bases, peroxide, catalysts, salts
    Chemicals,
}

pub const CATS: [(Cat, &str, &str); 9] = [
    (Cat::Structure, "Structure", "Metals, ceramics, glass, stone, ice, wax and plastic: build engines, vessels, vaults and labs"),
    (Cat::Burnable, "Explosives", "Wood, solid propellant, TNT, C4, dynamite, blasting caps and fuses"),
    (Cat::Powders, "Powders", "Grains that fall and pile up. Mix fuels with oxidisers for black powder, rocket candy, thermite, flash powder..."),
    (Cat::Liquids, "Liquids", "Water, fuels, oils, cryogenic liquids, lava and molten metal: they flow, pool, float and sink"),
    (Cat::Metals, "Metals", "Reactive metals: magnesium burns blinding white, sodium and potassium attack water, iron rusts"),
    (Cat::Chemicals, "Chemicals", "Acids, bases, peroxide, catalysts and salts: wet chemistry that fizzes, heats and smokes"),
    (Cat::Machines, "Machines", "Fans, igniters, vents and propellant feeds"),
    (Cat::Gases, "Gases", "Spray fuel, oxygen or air into the chamber"),
    (Cat::Tools, "Tools", "Torch, chill, vacuum and the eraser"),
];

pub struct MatInfo {
    pub name: &'static str,
    pub color: Color32,
    pub strength: f32,
    pub t_soft: f32,
    pub t_melt: f32,
    pub cond: f32,
    pub cap: f32,
    pub shock: Option<f32>,
}

impl MatInfo {
    /// Burnables never melt; they burn, pyrolyse or detonate instead.
    pub fn burns(&self) -> bool {
        self.t_melt > 50.0
    }
}

/// Must match the material tables in sim.wgsl (M_STR, M_TSOFT, M_TMELT, M_COND, M_CAP).
pub const MATS: [MatInfo; 10] = [
    MatInfo { name: "Steel", color: Color32::from_rgb(120, 125, 135), strength: 4.0, t_soft: 1.7, t_melt: 5.0, cond: 1.0, cap: 12.0, shock: None },
    MatInfo { name: "Aluminium", color: Color32::from_rgb(194, 199, 209), strength: 2.0, t_soft: 1.0, t_melt: 2.2, cond: 2.6, cap: 9.0, shock: None },
    MatInfo { name: "Copper", color: Color32::from_rgb(191, 110, 69), strength: 1.5, t_soft: 1.6, t_melt: 3.6, cond: 3.6, cap: 11.0, shock: None },
    MatInfo { name: "Titanium", color: Color32::from_rgb(148, 153, 173), strength: 6.5, t_soft: 2.0, t_melt: 5.6, cond: 0.5, cap: 10.0, shock: None },
    MatInfo { name: "Inconel", color: Color32::from_rgb(92, 110, 120), strength: 5.0, t_soft: 3.3, t_melt: 5.4, cond: 0.35, cap: 14.0, shock: None },
    MatInfo { name: "Ceramic", color: Color32::from_rgb(224, 214, 194), strength: 1.2, t_soft: 7.0, t_melt: 9.5, cond: 0.12, cap: 16.0, shock: Some(2.2) },
    MatInfo { name: "Cooled copper", color: Color32::from_rgb(158, 102, 77), strength: 3.0, t_soft: 1.6, t_melt: 3.6, cond: 3.6, cap: 11.0, shock: None },
    MatInfo { name: "Wood", color: Color32::from_rgb(115, 77, 41), strength: 1.5, t_soft: 3.0, t_melt: 99.0, cond: 0.15, cap: 10.0, shock: None },
    MatInfo { name: "Solid propellant", color: Color32::from_rgb(178, 158, 153), strength: 1.2, t_soft: 99.0, t_melt: 99.0, cond: 0.2, cap: 10.0, shock: None },
    MatInfo { name: "Explosive", color: Color32::from_rgb(217, 140, 64), strength: 1.0, t_soft: 99.0, t_melt: 99.0, cond: 0.2, cap: 10.0, shock: None },
];

pub struct Element {
    pub tool: Tool,
    /// four-letter code on the button, Powder Toy style
    pub code: &'static str,
    pub name: &'static str,
    pub cat: Cat,
    pub color: Color32,
    /// what it does in the game
    pub blurb: &'static str,
    /// a real-world fact
    pub fact: &'static str,
    /// extra search words
    pub tags: &'static str,
}

const fn m(k: usize) -> Color32 {
    MATS[k].color
}

pub const ELEMENTS: [Element; 20] = [
    Element { tool: Tool::Wall(0), code: "STEL", name: "Steel", cat: Cat::Structure, color: m(0),
        blurb: "Cheap all-rounder. Solid strength, but it softens early in hot sections.",
        fact: "Most gas cylinders and solid-rocket casings are steel: tough, weldable and forgiving. The Space Shuttle boosters were steel tubes 3.7 m wide.", tags: "metal wall iron" },
    Element { tool: Tool::Wall(1), code: "ALUM", name: "Aluminium", cat: Cat::Structure, color: m(1),
        blurb: "Light and spreads heat fast, but weak and melts early. Keep it away from flames.",
        fact: "Aluminium is a third as dense as steel, which is why airliners are made of it, but it loses about half its strength by 200 °C.", tags: "metal wall aluminum" },
    Element { tool: Tool::Wall(2), code: "COPR", name: "Copper", cat: Cat::Structure, color: m(2),
        blurb: "Best heat conductor. Weak, but great for spreading out hot spots.",
        fact: "Copper conducts heat about 8 times better than steel. Nearly every liquid-rocket combustion chamber is lined with a copper alloy.", tags: "metal wall" },
    Element { tool: Tool::Wall(3), code: "TITN", name: "Titanium", cat: Cat::Structure, color: m(3),
        blurb: "Strongest wall. Takes high pressure, but not extreme heat.",
        fact: "The SR-71 Blackbird was 85% titanium so it could survive 300 °C skin temperatures at Mach 3. The CIA secretly bought the ore from the USSR.", tags: "metal wall vault" },
    Element { tool: Tool::Wall(4), code: "INCN", name: "Inconel", cat: Cat::Structure, color: m(4),
        blurb: "Nickel superalloy for hot sections. Keeps its strength up to ~1000 K.",
        fact: "Inconel is used for turbine blades, exhaust manifolds and SpaceX's Merlin engine manifold; it stays strong while glowing red.", tags: "metal wall superalloy nickel" },
    Element { tool: Tool::Wall(5), code: "CRMC", name: "Ceramic", cat: Cat::Structure, color: m(5),
        blurb: "Survives 2800 K and insulates, but is brittle and cracks from thermal shock.",
        fact: "Space Shuttle tiles were silica ceramic. You could hold one by its edges seconds after it came out of a 1260 °C oven.", tags: "tile heat shield insulator" },
    Element { tool: Tool::Wall(6), code: "CCPR", name: "Cooled copper", cat: Cat::Structure, color: m(6),
        blurb: "Copper with coolant channels. Pulls heat away constantly, so it survives 3500 K flames unless the heat flux is extreme.",
        fact: "The Space Shuttle main engine's chamber had 390 coolant channels milled into a copper liner, with liquid hydrogen flowing through them at 3000 m/s.", tags: "regenerative cooling rocket chamber" },
    Element { tool: Tool::Wall(7), code: "WOOD", name: "Wood", cat: Cat::Burnable, color: m(7),
        blurb: "Above ~600 K it pyrolyses: gives off flammable gas that burns as flame, chars and slowly disappears. Smoky.",
        fact: "Wood itself doesn't burn. Heat breaks it down into flammable gases (pyrolysis) and it's those gases you see burning as flame.", tags: "log fire campfire burnable" },
    Element { tool: Tool::Wall(8), code: "APCP", name: "Solid propellant", cat: Cat::Burnable, color: m(8),
        blurb: "Carries its own oxidiser. Once lit (~750 K) it burns at exposed surfaces into 3000 K exhaust, faster at higher pressure.",
        fact: "Ammonium perchlorate composite propellant: each Shuttle booster burned 500 t of it in two minutes. Burn rate grows with pressure, r ∝ p^0.35.", tags: "solid rocket fuel grain apcp motor" },
    Element { tool: Tool::Wall(9), code: "EXPL", name: "Explosive", cat: Cat::Burnable, color: m(9),
        blurb: "Detonates above ~600 K or when hit by a strong shock (> 8 atm): becomes 3500 K high-pressure gas instantly. Neighbouring charges chain-react.",
        fact: "A detonation front moves faster than sound in the explosive itself (about 7 km/s for TNT-class charges), so the solid turns to gas before it can move.", tags: "tnt bomb blast demolition c4" },
    Element { tool: Tool::Part(scene::FAN), code: "FAN", name: "Fan", cat: Cat::Machines, color: Color32::from_rgb(240, 140, 40),
        blurb: "Pushes gas in its arrow direction (R rotates). Stack columns to make a compressor.",
        fact: "A modern turbofan's fan moves about a tonne of air per second; it makes most of the engine's thrust.", tags: "compressor blower turbine" },
    Element { tool: Tool::Part(scene::IGNITER), code: "IGNT", name: "Igniter", cat: Cat::Machines, color: Color32::from_rgb(230, 70, 40),
        blurb: "Glow plug: heats nearby gas and touching solids to ~3000 K while igniters are on (I).",
        fact: "Jet engines only fire their igniters during start-up; after that the flame holds itself in the combustor.", tags: "spark plug glow match lighter" },
    Element { tool: Tool::Part(scene::VENT), code: "VENT", name: "Vent", cat: Cat::Machines, color: Color32::from_rgb(110, 70, 140),
        blurb: "An opening to the outside atmosphere (1 atm, 300 K).",
        fact: "Pressure vessels carry burst disks: weak vents that pop open before the tank itself fails.", tags: "outlet exhaust atmosphere" },
    Element { tool: Tool::Part(scene::FUEL_IN), code: "CH4", name: "Fuel inlet", cat: Cat::Machines, color: Color32::from_rgb(115, 205, 40),
        blurb: "Feeds methane at the fuel feed pressure.",
        fact: "Methane is the fuel of SpaceX's Raptor and Blue Origin's BE-4: it burns cleaner than kerosene and can be made on Mars.", tags: "methane gas injector feed" },
    Element { tool: Tool::Part(scene::O2_IN), code: "O2IN", name: "Oxygen inlet", cat: Cat::Machines, color: Color32::from_rgb(50, 115, 255),
        blurb: "Feeds pure oxygen at the oxygen feed pressure, for rockets.",
        fact: "A methane rocket burns about 3.6 kg of liquid oxygen for every kilogram of fuel.", tags: "lox injector feed oxidiser" },
    Element { tool: Tool::Part(scene::AIR_IN), code: "AIR", name: "Air inlet", cat: Cat::Machines, color: Color32::from_rgb(150, 205, 230),
        blurb: "Feeds compressed air at the air feed pressure.",
        fact: "Air is 78% nitrogen, which just comes along for the ride, absorbing heat; that is why air flames are cooler than oxygen flames.", tags: "compressor feed" },
    Element { tool: Tool::Gas(2), code: "FUEL", name: "Fuel gas", cat: Cat::Gases, color: Color32::from_rgb(140, 215, 40),
        blurb: "Spray methane. A quick tap over air makes a burnable mix (5-15% methane); holding builds a rich cloud that only burns at its edges.",
        fact: "Methane only burns between 5% and 15% in air. Outside that range a spark does nothing.", tags: "methane ch4 spray" },
    Element { tool: Tool::Gas(3), code: "OXYG", name: "Oxygen", cat: Cat::Gases, color: Color32::from_rgb(70, 140, 255),
        blurb: "Spray pure oxygen: makes flames hotter and faster.",
        fact: "In pure oxygen a methane flame reaches about 3000 K, compared with about 2200 K in air.", tags: "o2 spray" },
    Element { tool: Tool::Gas(5), code: "TRCH", name: "Torch", cat: Cat::Tools, color: Color32::from_rgb(255, 120, 30),
        blurb: "Heats gas to 2700 K and the surface of solids: lights any fuel mixture, wood, propellant or explosive it touches.",
        fact: "An oxy-acetylene torch reaches 3500 K, hot enough to cut steel by burning it.", tags: "fire flame ignite heat match lighter" },
    Element { tool: Tool::Erase, code: "ERAS", name: "Eraser", cat: Cat::Tools, color: Color32::from_rgb(70, 70, 78),
        blurb: "Removes walls and machines (E). Right-drag erases with any tool.",
        fact: "Every engineer's favourite tool.", tags: "delete remove clear" },
];

/// Extra gas tools (air / chill / vacuum) that live in the browser but are less important.
pub const EXTRA: [Element; 4] = [
    Element { tool: Tool::Gas(4), code: "AIR+", name: "Air", cat: Cat::Gases, color: Color32::from_rgb(130, 160, 190),
        blurb: "Add air (raises pressure).", fact: "Sea-level air weighs 1.2 kg per cubic metre.", tags: "pressure spray" },
    Element { tool: Tool::Gas(6), code: "CHIL", name: "Chill", cat: Cat::Tools, color: Color32::from_rgb(80, 200, 230),
        blurb: "Cools gas down.", fact: "Liquid nitrogen boils at 77 K (-196 °C).", tags: "cool cold freeze" },
    Element { tool: Tool::Gas(7), code: "VACU", name: "Vacuum", cat: Cat::Tools, color: Color32::from_rgb(90, 90, 100),
        blurb: "Sucks gas out.", fact: "Low Earth orbit is a better vacuum than any lab on Earth could make in the 1960s.", tags: "suck remove gas" },
    Element { tool: Tool::Gas(8), code: "MIX", name: "Mix", cat: Cat::Tools, color: Color32::from_rgb(150, 150, 165),
        blurb: "Stirs powders and liquids under the brush into an intimate mixture. Reactions need grains in contact, so better-mixed = faster burning.", fact: "Black powder is ground and mixed for hours: the finer and more intimate the mix, the faster it burns.", tags: "mix stir blend combine shuffle" },
];

const fn mt(id: u32, code: &'static str, name: &'static str, cat: Cat, rgb: (u8, u8, u8), blurb: &'static str, fact: &'static str, tags: &'static str) -> Element {
    Element { tool: Tool::Matter(id), code, name, cat, color: Color32::from_rgb(rgb.0, rgb.1, rgb.2), blurb, fact, tags }
}

/// Powders, liquids and placeable solids (tile 8, element id in the material byte).
/// Must match the tables in matter.wgsl (`E_*`, generated by the chem agent) and `MATTER_INFO` below.
pub const MATTER: [Element; 56] = [
    // ---- powders ----
    mt(10, "SAND", "Sand", Cat::Powders, (204, 179, 115), "Inert. Piles up; smothers fires and tamps charges. Melts into glass above ~1700 K.", "Sand is mostly quartz (SiO2). Glass makers melt it at about 1700 °C; lightning striking a beach makes glass tubes called fulgurites.", "powder inert silica glass"),
    mt(11, "CHAR", "Charcoal", Cat::Powders, (33, 31, 28), "Fuel. Glows and burns in air above ~700 K. A cloud of it burns explosively.", "Charcoal is nearly pure carbon: wood heated without air until everything else has cooked off.", "carbon fuel black powder"),
    mt(12, "SULF", "Sulfur", Cat::Powders, (230, 214, 51), "Fuel. Ignites at only ~505 K, so it helps other mixes catch.", "Sulfur lowers black powder's ignition temperature and makes it burn faster. It burns with a blue flame into choking SO2.", "brimstone black powder fuel"),
    mt(13, "KNO3", "Saltpeter", Cat::Powders, (237, 237, 242), "Oxidiser: releases O2 above ~673 K. With charcoal (+ sulfur) = black powder; with sugar = rocket candy; with iron filings = sparks.", "Black powder is 75% saltpeter, 15% charcoal and 10% sulfur, a recipe unchanged for 700 years.", "potassium nitrate oxidiser black powder rocket candy"),
    mt(14, "SUGR", "Sugar", Cat::Powders, (245, 237, 219), "Fuel. With saltpeter it makes rocket candy. Sugar dust in air can explode.", "Rocket candy (KNO3 + sugar) has powered amateur rockets past 10 km. In 2008 a sugar-dust explosion destroyed a refinery in Georgia, USA.", "sucrose fuel rocket candy dust"),
    mt(64, "FLOR", "Flour", Cat::Powders, (245, 240, 224), "Harmless in a heap, but a dust cloud of it burns explosively. Fine dust hangs in the air.", "Grain-silo and flour-mill dust explosions have levelled buildings since the 1700s; a 1878 Minneapolis mill blast killed 18 people.", "dust explosion wheat grain silo powder fuel"),
    mt(37, "COAL", "Coal", Cat::Powders, (20, 20, 23), "Slow, hot, smoky fuel (ignites ~720 K). Coal dust is explosive.", "Coal-dust explosions were the great killer in mines: the 1906 Courrières disaster in France killed 1099 miners.", "fuel carbon smoke dust mine"),
    mt(16, "NH4P", "Ammonium perchlorate", Cat::Powders, (214, 230, 250), "Oxidiser. With aluminium or magnesium powder or sugar: composite rocket propellant that burns faster under pressure.", "APCP propellant is about 70% ammonium perchlorate by mass.", "ap oxidiser apcp rocket"),
    mt(47, "KCLO", "Potassium perchlorate", Cat::Powders, (245, 245, 250), "Strong oxidiser. With aluminium or magnesium powder = flash powder: explodes even in the open.", "Flash powder (KClO4 + Al) makes the bang in firecrackers and salutes; it burns in about a millisecond.", "kclo4 flash powder oxidiser firework"),
    mt(60, "KMNO", "Potassium permanganate", Cat::Chemicals, (102, 26, 107), "Purple oxidiser. With glycerin it bursts into flame by itself after a few seconds. Catalyses peroxide.", "Permanganate + glycerin is the classic 'fire without a match' demo; survival kits carry permanganate for this.", "kmno4 oxidiser glycerin purple"),
    mt(17, "RUST", "Rust", Cat::Powders, (143, 51, 31), "Iron oxide. With aluminium (or magnesium) powder = thermite: hard to light (~1350 K) but makes molten iron. Catalyses peroxide.", "Thermite burns at about 2500 °C and is used to weld railway tracks together.", "iron oxide thermite"),
    mt(59, "CUO", "Copper oxide", Cat::Powders, (26, 23, 23), "Black oxide. With aluminium powder = copper thermite: far faster than iron thermite, sprays molten copper.", "Copper thermite is so fast it is practically explosive; it is used in some fireworks and military igniters.", "cuo thermite oxide copper"),
    mt(18, "ANIT", "Ammonium nitrate", Cat::Powders, (247, 245, 230), "Oxidiser. With diesel, kerosene or charcoal = ANFO, which only smoulders unless a strong shock sets it off.", "ANFO is the world's most used mining explosive. It needs a booster charge to detonate.", "anfo fertiliser oxidiser"),
    mt(57, "GUNP", "Gunpowder", Cat::Powders, (51, 51, 56), "Ready-mixed black powder. Lights from a flame (~700 K) and burns fast with lots of smoke; confined, it bursts.", "Gunpowder was invented in 9th-century China by alchemists looking for an elixir of life.", "black powder premix firework"),
    mt(48, "NCEL", "Nitrocellulose", Cat::Powders, (219, 214, 184), "Smokeless powder: self-oxidising, lights at only ~440 K and burns very fast and clean.", "Guncotton was discovered in 1846 when Christian Schönbein wiped up spilled acid with his wife's cotton apron, which then flashed away.", "smokeless powder guncotton propellant"),
    mt(28, "SALT", "Salt", Cat::Chemicals, (240, 240, 235), "Dissolves in water into brine. Melts ice (salt water freezes only at 252 K).", "Roads are salted because brine stays liquid down to about -21 °C.", "nacl brine sodium chloride"),
    mt(46, "MNO2", "Manganese dioxide", Cat::Chemicals, (41, 36, 33), "Catalyst: makes hydrogen peroxide decompose violently into steam and oxygen. Not used up.", "MnO2 + concentrated peroxide is the 'elephant toothpaste' demo, and the catalyst in early peroxide rocket motors.", "catalyst peroxide elephant toothpaste"),
    mt(58, "DICE", "Dry ice", Cat::Powders, (235, 242, 250), "Solid CO2 at 195 K. Sublimates into heavy cold CO2 gas that smothers fires.", "Dry ice skips the liquid state at normal pressure; CO2 fire extinguishers spray a snow of it.", "co2 carbon dioxide extinguisher cold"),
    mt(27, "SNOW", "Snow", Cat::Powders, (242, 247, 255), "Fluffy ice crystals. Drifts slowly down, melts into water at 273 K.", "Fresh snow is about 90% air, which is why it insulates so well.", "ice cold frozen"),
    mt(25, "ASH", "Ash", Cat::Powders, (115, 112, 107), "Inert residue left after burning.", "Wood ash is mostly calcium carbonate and potassium salts: potash.", "slag residue"),
    // ---- metals ----
    mt(15, "ALPW", "Aluminium powder", Cat::Metals, (191, 196, 204), "Metal fuel that burns at ~3000 K. With rust = thermite; with perchlorate = APCP or flash powder. Fizzes in acid and lye.", "The Space Shuttle boosters' propellant was 16% aluminium powder.", "aluminum metal fuel thermite apcp"),
    mt(39, "MG", "Magnesium", Cat::Metals, (204, 209, 214), "Lights at ~780 K and burns blinding white at 3100 K. Water and CO2 make it worse. Fizzes in acid.", "Magnesium flares and old camera flashbulbs burn magnesium; a burning Mg fire splits water into hydrogen.", "metal flare white thermite ribbon"),
    mt(40, "FE", "Iron filings", Cat::Metals, (77, 77, 82), "Rusts when wet (faster in salt water). Burns as sparks when hot; with saltpeter makes sparklers.", "Steel wool burns in air with orange sparks; sparkler sparks are burning iron filings.", "iron metal rust sparkler"),
    mt(41, "NA", "Sodium", Cat::Metals, (199, 199, 189), "Floats on water and reacts: hydrogen + heat + lye. Burns with a yellow flame.", "Sodium is soft enough to cut with a knife and is stored under oil because it reacts with moisture in the air.", "alkali metal water hydrogen"),
    mt(42, "K", "Potassium", Cat::Metals, (184, 184, 194), "Like sodium but more violent: lights its own hydrogen and spits burning bits.", "Potassium hits water so fast that the metal surface explodes into a spray before the hydrogen even ignites (a Coulomb explosion).", "alkali metal water hydrogen lilac"),
    // ---- liquids ----
    mt(19, "WATR", "Water", Cat::Liquids, (38, 89, 217), "Boils at 373 K and soaks up huge amounts of heat; freezes at 273 K. Touching molten metal or burning oil it flashes into a steam explosion.", "Boiling 1 kg of water absorbs 2.3 MJ, about as much as heating it from 0 to 100 °C five times.", "h2o extinguish liquid"),
    mt(29, "BRIN", "Brine", Cat::Liquids, (77, 122, 204), "Salt water: freezes at 252 K, boils dry into salt crystals, rusts iron fast.", "Sea water is about 3.5% salt; the Dead Sea is ten times saltier.", "salt water sea"),
    mt(20, "KERO", "Kerosene", Cat::Liquids, (194, 140, 51), "Fuel. Floats on water; its vapour burns as a pool fire.", "Kerosene (RP-1) fuelled the Saturn V's first stage and the Falcon 9.", "rp1 jet fuel oil liquid anfo"),
    mt(31, "GASO", "Gasoline", Cat::Liquids, (237, 204, 115), "Very volatile: evaporates at room temperature into a flammable vapour cloud that flashes back to the pool.", "Gasoline's flash point is about -43 °C: there is always enough vapour over it to burn.", "petrol fuel volatile vapour"),
    mt(32, "DESL", "Diesel", Cat::Liquids, (158, 128, 46), "Heavy fuel: hard to light (flash point ~325 K) but burns long and smoky. With ammonium nitrate = ANFO.", "Diesel engines need no spark: compressing air heats it enough to ignite the injected fuel.", "fuel oil anfo smoky"),
    mt(33, "OIL", "Cooking oil", Cat::Liquids, (217, 179, 56), "Heats past 600 K and its vapour catches fire. Water on burning oil flashes to steam and throws flaming oil everywhere.", "Grease fires cause most kitchen fires; firefighters' demos of a cup of water on a burning pan make a 3-metre fireball.", "grease fire fat kitchen"),
    mt(34, "NPLM", "Napalm", Cat::Liquids, (148, 102, 41), "Sticky gelled fuel: barely flows, sticks to things and burns long, hot and sooty.", "Napalm is gasoline thickened with a gelling agent so it sticks and burns for minutes instead of seconds.", "gel fuel sticky incendiary"),
    mt(21, "ETOH", "Ethanol", Cat::Liquids, (204, 219, 235), "Evaporates easily and burns with a clean, almost invisible flame.", "The V-2 rocket burned 75% ethanol with liquid oxygen.", "alcohol fuel liquid"),
    mt(61, "GLYC", "Glycerin", Cat::Liquids, (235, 235, 214), "Thick, sweet liquid. With potassium permanganate it ignites on its own.", "Glycerin plus nitric acid makes nitroglycerin; on its own it is used in soap and food.", "glycerol permanganate"),
    mt(22, "LOX", "Liquid oxygen", Cat::Liquids, (140, 191, 255), "90 K. With any fuel it is violently explosive.", "LOX soaked into charcoal (oxyliquit) was once used as a mining explosive.", "lox oxygen cryogenic oxyliquit"),
    mt(30, "LN2", "Liquid nitrogen", Cat::Liquids, (209, 224, 245), "77 K. Boils into nitrogen and freezes water it touches.", "Liquid nitrogen boils at -196 °C; a litre makes almost 700 litres of gas.", "cryogenic cold freeze nitrogen"),
    mt(23, "NITR", "Nitroglycerin", Cat::Liquids, (242, 230, 153), "Detonates from heat (~490 K) or a shock. Handle with care.", "Alfred Nobel invented dynamite to tame nitroglycerin by soaking it into clay.", "ng explosive dynamite liquid"),
    mt(55, "LAVA", "Lava", Cat::Liquids, (255, 89, 20), "1300 K molten rock: flows slowly, sets things alight, cools into stone. Steam explosions in water.", "Basalt lava erupts at about 1100-1200 °C; Hawaiian lava entering the sea builds new land in clouds of steam.", "magma volcano molten rock"),
    mt(24, "MFE", "Molten iron", Cat::Liquids, (255, 115, 26), "2800 K liquid metal: flows, sets things alight and solidifies into steel. Water touching it explodes into steam.", "Iron melts at 1811 K; blast furnaces tap it out at around 1800 K.", "lava metal thermite"),
    mt(54, "MGLS", "Molten glass", Cat::Liquids, (255, 140, 46), "Glowing, very viscous; cools into glass below ~1350 K.", "Glassblowers work glass at about 1100 °C, when it is like thick honey.", "lava glass molten"),
    mt(36, "MWAX", "Molten wax", Cat::Liquids, (247, 235, 179), "Liquid wax: its vapour burns (that is how a candle works). Freezes back below 324 K.", "A candle flame burns wax vapour: the wick draws up molten wax that the flame boils.", "candle paraffin"),
    mt(63, "MPLS", "Molten plastic", Cat::Liquids, (77, 140, 217), "Drips and burns with thick black smoke.", "Burning plastic drips spread fires downward; its smoke is dense with soot.", "melted plastic smoke"),
    // ---- chemicals (liquids) ----
    mt(44, "HCL", "Hydrochloric acid", Cat::Chemicals, (209, 230, 158), "Eats metals (magnesium fast, aluminium, iron slowly) giving hydrogen; dissolves rust; with lye gives hot salt water.", "Your stomach makes dilute hydrochloric acid, about pH 1.5-3.5.", "acid hcl muriatic hydrogen"),
    mt(43, "LYE", "Lye", Cat::Chemicals, (184, 199, 158), "Sodium hydroxide solution. Dissolves aluminium (hydrogen); neutralises acids with heat.", "Drain cleaner is lye plus aluminium bits: the hydrogen fizz and heat stir up the clog.", "naoh base alkali caustic"),
    mt(45, "H2O2", "Hydrogen peroxide", Cat::Chemicals, (199, 224, 245), "90% HTP. On a catalyst (manganese dioxide, permanganate, rust) it decomposes into 1000 K steam + oxygen: a monopropellant.", "The Bell rocket belt and the Me 163 rocket fighter flew on hydrogen peroxide.", "htp peroxide monopropellant oxidiser"),
    // ---- solids ----
    mt(26, "ICE", "Ice", Cat::Structure, (199, 230, 250), "Frozen water at 255 K: melts at 273 K (salt melts it colder).", "Ice is about 9% less dense than water, which is why lakes freeze from the top down.", "frozen water cold"),
    mt(53, "GLAS", "Glass", Cat::Structure, (189, 219, 224), "Transparent, inert, melts above ~1450 K. Good for lab beakers.", "Glass is an amorphous solid: its atoms are as disordered as a liquid's, frozen in place.", "beaker window silica"),
    mt(56, "STNE", "Stone", Cat::Structure, (69, 66, 66), "Basalt rock: does not burn, melts into lava above ~1230 K.", "Most of the ocean floor is basalt, made from lava that cooled underwater.", "rock basalt"),
    mt(35, "WAX", "Wax", Cat::Structure, (237, 230, 204), "Paraffin: melts at ~330 K into a liquid whose vapour burns. Makes heat-triggered plugs.", "Paraffin wax melts at 46-68 °C and releases 42 MJ/kg when burned, about as much as diesel.", "candle paraffin plug"),
    mt(62, "PLST", "Plastic", Cat::Structure, (51, 128, 217), "Polyethylene: melts at ~430 K, then drips and burns very smokily.", "Polyethylene is the world's most common plastic, basically a solid very long-chain paraffin.", "polyethylene pe melt"),
    mt(38, "PAPR", "Paper", Cat::Burnable, (237, 232, 217), "Lights at 451 °F (506 K) and burns fast, leaving ash.", "Ray Bradbury's 'Fahrenheit 451' is named after paper's auto-ignition temperature.", "cardboard fuel tinder"),
    mt(65, "FUSE", "Fuse", Cat::Burnable, (128, 87, 51), "Black-match cord: burns steadily along its length and lights whatever is at the end.", "Bickford's safety fuse (1831) burns at a steady ~1 cm/s, which cut mining deaths dramatically.", "wick timer black match"),
    mt(49, "TNT", "TNT", Cat::Burnable, (230, 209, 140), "Insensitive high explosive: a flame only makes it burn (very sooty). Needs a strong shock (> 25 atm) to detonate.", "TNT is so insensitive it was first used as a yellow dye; it was 30 years before anyone realised it was an explosive.", "trinitrotoluene high explosive"),
    mt(50, "C4", "C4", Cat::Burnable, (237, 235, 219), "Plastic explosive. Burns quietly if lit; detonates only from a blasting cap or another blast (> 30 atm).", "Soldiers in Vietnam lit small pieces of C4 to heat rations: it just burns unless shocked.", "rdx plastic explosive"),
    mt(51, "DYNA", "Dynamite", Cat::Burnable, (199, 77, 56), "Nitroglycerin in clay: safer to handle, but detonates from a blast (> 7 atm) or when a fire heats it.", "Nobel's dynamite (1867) made blasting tunnels like the Gotthard possible and funded the Nobel prizes.", "nitroglycerin stick explosive"),
    mt(52, "CAP", "Blasting cap", Cat::Burnable, (184, 168, 102), "Small, very sensitive primary explosive (lead azide): any flame, heat or bump sets it off. Use it to detonate TNT, C4 and ANFO.", "Primary explosives like lead azide detonate when merely lit, so tiny amounts are used to start big secondary charges.", "detonator primer booster"),
];

/// Recipes shown on the help card: (name, ingredients, what happens). Ingredient names match element names.
pub const RECIPES: [(&str, &str, &str); 24] = [
    ("Black powder", "Saltpeter + charcoal (+ sulfur)", "Burns fast and hot; confined, it bursts"),
    ("Rocket candy", "Saltpeter + sugar", "Steady, smoky rocket propellant"),
    ("APCP", "Ammonium perchlorate + aluminium powder", "Composite rocket propellant, faster under pressure"),
    ("Flash powder", "Potassium perchlorate + aluminium powder (or magnesium)", "Explodes even in the open, with a white flash"),
    ("Thermite", "Rust + aluminium powder", "Hard to light; makes molten iron that burns through steel. Light it with magnesium"),
    ("Magnesium thermite", "Rust + magnesium", "Lights more easily than aluminium thermite"),
    ("Copper thermite", "Copper oxide + aluminium powder", "Almost explosive; sprays molten copper"),
    ("ANFO", "Ammonium nitrate + diesel (or kerosene, charcoal)", "Only smoulders, unless a strong shock (blasting cap) sets it off"),
    ("Oxyliquit", "Liquid oxygen + charcoal", "Violently explosive"),
    ("Sparklers", "Saltpeter + iron filings", "Showers of orange sparks"),
    ("Fire without a match", "Potassium permanganate + glycerin", "Smokes, then bursts into flame after a few seconds"),
    ("Elephant toothpaste", "Hydrogen peroxide + manganese dioxide", "Violent decomposition into hot steam and oxygen"),
    ("Peroxide rocket", "Hydrogen peroxide + manganese dioxide (or potassium permanganate) in a chamber", "1000 K steam jet: a monopropellant"),
    ("Alkali + water", "Sodium (or potassium) + water", "Hydrogen, heat and lye; potassium lights its own hydrogen"),
    ("Metal + acid", "Magnesium (or aluminium, iron filings) + hydrochloric acid", "Fizzes out hydrogen gas; light it for a pop"),
    ("Neutralisation", "Hydrochloric acid + lye", "Hot salt water (brine)"),
    ("Drain cleaner", "Aluminium powder + lye", "Hydrogen and heat"),
    ("Brine", "Salt + water", "Salt dissolves; brine freezes at 252 K and boils dry into salt"),
    ("Road salt", "Salt + ice", "Ice melts down to 252 K"),
    ("Rust", "Iron filings + water (or brine)", "Slowly rusts; salt water is faster"),
    ("Steam explosion", "Water + molten iron (or lava, burning cooking oil)", "Water flashes into steam: violent burst"),
    ("Dust explosion", "Flour (or coal, sugar, aluminium powder) dust cloud + spark", "A suspended dust cloud burns explosively"),
    ("Glass", "Sand heated above 1700 K", "Molten glass, cools into glass"),
    ("Booster", "Blasting cap + TNT (or C4, ANFO)", "A cap's small detonation sets off the main charge"),
];

/// Physical data for each matter element (mirrors matter.wgsl tables, for the inspector / encyclopedia).
pub struct MatterInfo {
    pub id: u32,
    /// 0 static solid, 1 powder, 2 liquid
    pub phase: u8,
    /// density relative to water
    pub density: f32,
    /// melting point (K) or None
    pub melt_k: Option<f32>,
    /// boiling / sublimation / decomposition point (K) or None
    pub boil_k: Option<f32>,
    /// ignition temperature in air or when lit (K) or None
    pub ignite_k: Option<f32>,
    /// shock pressure (atm) that detonates it, if it is an explosive
    pub detonate_atm: Option<f32>,
    /// short hazard line
    pub hazard: &'static str,
    /// what it releases when it reacts (in game gas terms)
    pub gives: &'static str,
    /// specific heat capacity, J/(kg K) (at the phase shown)
    pub cp: f32,
    /// lower heat of combustion in air, MJ/kg (None = not a fuel)
    pub hc_mj: Option<f32>,
    /// heat of explosion / decomposition / self-reaction, MJ/kg (energetics, peroxide, oxidisers)
    pub hex_mj: Option<f32>,
    /// latent heat of fusion, kJ/kg
    pub l_fus_kj: Option<f32>,
    /// latent heat of vaporisation / sublimation, kJ/kg
    pub l_vap_kj: Option<f32>,
}

impl MatterInfo {
    const fn si(mut self, cp: f32, hc_mj: Option<f32>, hex_mj: Option<f32>, l_fus_kj: Option<f32>, l_vap_kj: Option<f32>) -> Self {
        self.cp = cp;
        self.hc_mj = hc_mj;
        self.hex_mj = hex_mj;
        self.l_fus_kj = l_fus_kj;
        self.l_vap_kj = l_vap_kj;
        self
    }
    /// density in kg/m3
    pub fn density_kg_m3(&self) -> f32 {
        self.density * 1000.0
    }
    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            0 => "solid",
            1 => "powder",
            _ => "liquid",
        }
    }
}

const fn mi(id: u32, phase: u8, density: f32, melt_k: Option<f32>, boil_k: Option<f32>, ignite_k: Option<f32>, detonate_atm: Option<f32>, hazard: &'static str, gives: &'static str) -> MatterInfo {
    MatterInfo { id, phase, density, melt_k, boil_k, ignite_k, detonate_atm, hazard, gives, cp: 0.0, hc_mj: None, hex_mj: None, l_fus_kj: None, l_vap_kj: None }
}

pub const MATTER_INFO: [MatterInfo; 56] = [
    mi(10, 1, 1.6, Some(1710.0), None, None, None, "Inert", "Molten glass when very hot").si(830.0, None, None, None, None),
    mi(11, 1, 1.3, None, None, Some(690.0), None, "Flammable; dust cloud explodes", "CO2 + heat").si(1000.0, Some(29.6), None, None, None),
    mi(12, 1, 1.9, None, None, Some(510.0), None, "Flammable", "Products + heat").si(710.0, Some(9.2), None, Some(54.0), None),
    mi(13, 1, 2.1, None, Some(675.0), None, None, "Oxidiser", "Oxygen when hot").si(950.0, None, None, Some(95.0), None),
    mi(14, 1, 1.6, None, None, Some(540.0), None, "Flammable; dust cloud explodes", "CO2 + steam").si(1240.0, Some(16.5), None, None, None),
    mi(15, 1, 2.7, None, None, Some(930.0), None, "Burns at 3000 K; dust explodes", "Alumina smoke").si(900.0, Some(31.0), None, Some(397.0), None),
    mi(16, 1, 1.95, None, Some(570.0), None, None, "Strong oxidiser", "Hot products").si(1280.0, None, Some(1.1), None, None),
    mi(17, 1, 5.2, None, None, None, None, "Thermite ingredient", "Molten iron (thermite)").si(650.0, None, None, None, None),
    mi(18, 1, 1.7, None, Some(480.0), None, Some(12.0), "Oxidiser; ANFO with fuel", "Products when hot").si(1700.0, None, Some(1.6), None, None),
    mi(19, 2, 1.0, Some(273.0), Some(373.0), None, None, "Steam explosion on molten metal", "Steam").si(4186.0, None, None, Some(334.0), Some(2257.0)),
    mi(20, 2, 0.8, None, Some(450.0), None, None, "Flammable vapour", "Fuel vapour").si(2010.0, Some(43.1), None, None, Some(251.0)),
    mi(21, 2, 0.79, None, Some(351.0), None, None, "Flammable vapour", "Fuel vapour").si(2440.0, Some(29.7), None, Some(108.0), Some(841.0)),
    mi(22, 2, 1.14, None, Some(90.0), None, None, "Explosive with any fuel", "Oxygen").si(1700.0, None, None, Some(13.9), Some(213.0)),
    mi(23, 2, 1.6, None, None, Some(490.0), Some(3.0), "Detonates from heat or shock", "Detonation products").si(1350.0, None, Some(6.3), None, None),
    mi(24, 2, 7.0, Some(1800.0), None, None, None, "2800 K; ignites everything", "Heat").si(820.0, None, None, Some(247.0), None),
    mi(25, 1, 1.2, None, None, None, None, "Inert", "-").si(800.0, None, None, None, None),
    mi(26, 0, 0.92, Some(273.0), None, None, None, "Cold", "Water when melted").si(2050.0, None, None, Some(334.0), None),
    mi(27, 1, 0.3, Some(273.0), None, None, None, "Cold", "Water when melted").si(2090.0, None, None, Some(334.0), None),
    mi(28, 1, 2.16, None, None, None, None, "Dissolves in water", "Brine").si(880.0, None, None, Some(482.0), None),
    mi(29, 2, 1.2, Some(252.0), Some(375.0), None, None, "Corrodes iron", "Steam; salt when boiled dry").si(3300.0, None, None, None, Some(2257.0)),
    mi(30, 2, 0.81, None, Some(77.0), None, None, "Cryogenic: frostbite, freezes water", "Nitrogen").si(2040.0, None, None, Some(25.7), Some(199.0)),
    mi(31, 2, 0.74, None, Some(360.0), None, None, "Extremely flammable vapour", "Fuel vapour").si(2220.0, Some(46.4), None, None, Some(350.0)),
    mi(32, 2, 0.83, None, Some(540.0), None, None, "Flammable, smoky", "Fuel vapour + soot").si(2050.0, Some(45.5), None, None, Some(250.0)),
    mi(33, 2, 0.92, None, Some(600.0), None, None, "Grease fire: never use water", "Fuel vapour + smoke").si(1970.0, Some(37.0), None, None, None),
    mi(34, 2, 0.9, None, Some(660.0), Some(540.0), None, "Sticky incendiary", "Sooty flames").si(2100.0, Some(42.0), None, None, None),
    mi(35, 0, 0.9, Some(330.0), None, None, None, "Melts, then burns", "Molten wax").si(2500.0, Some(42.0), None, Some(200.0), None),
    mi(36, 2, 0.8, Some(324.0), Some(600.0), None, None, "Hot liquid; vapour burns", "Fuel vapour").si(2900.0, Some(42.0), None, Some(200.0), None),
    mi(37, 1, 1.35, None, None, Some(720.0), None, "Smoky; dust explodes", "CO2 + soot").si(1300.0, Some(27.0), None, None, None),
    mi(38, 0, 0.8, None, None, Some(506.0), None, "Flammable", "Smoke, ash").si(1340.0, Some(17.0), None, None, None),
    mi(39, 1, 1.74, None, None, Some(780.0), None, "Burns 3100 K; water/CO2 make it worse", "MgO smoke, hydrogen from water").si(1020.0, Some(24.7), None, Some(349.0), None),
    mi(40, 1, 3.5, Some(1800.0), None, Some(840.0), None, "Rusts when wet", "Sparks; rust").si(450.0, Some(7.4), None, Some(247.0), None),
    mi(41, 1, 0.97, Some(371.0), None, Some(480.0), None, "Reacts with water", "Hydrogen + lye").si(1230.0, Some(9.1), None, Some(113.0), None),
    mi(42, 1, 0.86, Some(337.0), None, Some(420.0), None, "Reacts violently with water", "Hydrogen + lye, burning bits").si(757.0, Some(4.6), None, Some(61.0), None),
    mi(43, 2, 1.3, None, Some(390.0), None, None, "Caustic", "Brine with acid; hydrogen with aluminium").si(3200.0, None, None, None, None),
    mi(44, 2, 1.18, None, Some(360.0), None, None, "Corrosive, fuming", "Hydrogen with metals; brine with lye").si(2500.0, None, None, None, None),
    mi(45, 2, 1.39, None, Some(420.0), None, None, "Decomposes violently on catalysts", "Oxygen + steam at 1000 K").si(2620.0, None, Some(2.9), Some(368.0), Some(1520.0)),
    mi(46, 1, 5.0, None, None, None, None, "Catalyst", "-").si(630.0, None, None, None, None),
    mi(47, 1, 2.52, None, Some(660.0), None, None, "Strong oxidiser; flash powder", "Oxygen when hot").si(810.0, None, None, None, None),
    mi(48, 1, 1.0, None, None, Some(444.0), None, "Very fast burning", "Clean hot products").si(1400.0, Some(10.0), Some(4.0), None, None),
    mi(49, 0, 1.65, Some(354.0), None, Some(570.0), Some(25.0), "Needs a booster to detonate", "Sooty products").si(1130.0, Some(15.0), Some(4.6), Some(98.0), None),
    mi(50, 0, 1.6, None, None, Some(660.0), Some(30.0), "Needs a blasting cap", "Detonation products").si(1200.0, Some(9.8), Some(6.0), None, None),
    mi(51, 0, 1.4, None, None, Some(570.0), Some(7.0), "Detonates in fire or from a blast", "Detonation products").si(1300.0, None, Some(5.0), None, None),
    mi(52, 0, 2.0, None, None, Some(600.0), Some(4.0), "Primary explosive: very sensitive", "Detonation").si(470.0, None, Some(1.6), None, None),
    mi(53, 0, 2.5, Some(1440.0), None, None, None, "Inert", "-").si(840.0, None, None, None, None),
    mi(54, 2, 2.4, Some(1350.0), None, None, None, "1700 K", "Heat").si(1300.0, None, None, None, None),
    mi(55, 2, 2.6, Some(1080.0), None, None, None, "1300 K; steam explosions", "Heat").si(1200.0, None, None, Some(400.0), None),
    mi(56, 0, 2.9, Some(1230.0), None, None, None, "Inert", "Lava when molten").si(840.0, None, None, Some(400.0), None),
    mi(57, 1, 1.7, None, None, Some(690.0), None, "Flammable; bursts when confined", "Hot products + thick smoke").si(1100.0, None, Some(2.8), None, None),
    mi(58, 1, 1.5, None, Some(195.0), None, None, "Cryogenic; displaces oxygen", "Cold CO2").si(950.0, None, None, None, Some(571.0)),
    mi(59, 1, 6.3, None, None, None, None, "Thermite ingredient", "Molten copper (thermite)").si(530.0, None, None, None, None),
    mi(60, 1, 2.7, None, Some(510.0), None, None, "Oxidiser; ignites glycerin", "Oxygen when hot").si(750.0, None, None, None, None),
    mi(61, 2, 1.26, None, Some(560.0), None, None, "Ignites with permanganate", "Fuel vapour").si(2430.0, Some(16.0), None, Some(199.0), Some(974.0)),
    mi(62, 0, 0.95, Some(435.0), None, None, None, "Melts and drips", "Molten plastic").si(1900.0, Some(46.3), None, Some(230.0), None),
    mi(63, 2, 0.9, Some(420.0), Some(690.0), None, None, "Burning drips, black smoke", "Sooty fuel vapour").si(2500.0, Some(46.3), None, Some(230.0), None),
    mi(64, 1, 0.6, None, None, Some(660.0), None, "Dust cloud explodes", "CO2 + steam").si(1700.0, Some(15.5), None, None, None),
    mi(65, 0, 1.2, None, None, Some(540.0), None, "Burns steadily", "Hot products").si(1200.0, None, Some(2.0), None, None),
];

/// SI data sheet for the wall materials 0-9 (real reference values; the sim uses scaled versions in sim.wgsl).
pub struct WallSi {
    /// kg/m3
    pub density: f32,
    /// J/(kg K)
    pub cp: f32,
    /// thermal conductivity W/(m K)
    pub k: f32,
    /// melting point K (burnables: ignition / decomposition point)
    pub melt_k: f32,
    /// yield / tensile strength MPa (None for burnables)
    pub strength_mpa: Option<f32>,
    /// heat of combustion / explosion MJ/kg for burnables
    pub heat_mj: Option<f32>,
}

const fn ws(density: f32, cp: f32, k: f32, melt_k: f32, strength_mpa: Option<f32>, heat_mj: Option<f32>) -> WallSi {
    WallSi { density, cp, k, melt_k, strength_mpa, heat_mj }
}

/// Indexed by wall material id (0 steel .. 9 explosive).
pub const WALL_SI: [WallSi; 10] = [
    ws(7850.0, 490.0, 50.0, 1700.0, Some(250.0), None),     // structural steel
    ws(2700.0, 900.0, 205.0, 933.0, Some(270.0), None),     // aluminium 6061-T6
    ws(8960.0, 385.0, 400.0, 1358.0, Some(70.0), None),     // copper (annealed)
    ws(4500.0, 520.0, 22.0, 1941.0, Some(880.0), None),     // Ti-6Al-4V
    ws(8440.0, 444.0, 10.0, 1610.0, Some(1030.0), None),    // Inconel 718
    ws(3950.0, 880.0, 30.0, 2345.0, Some(300.0), None),     // alumina ceramic (flexural)
    ws(8960.0, 385.0, 400.0, 1358.0, Some(70.0), None),     // regeneratively cooled copper
    ws(600.0, 1700.0, 0.15, 573.0, Some(40.0), Some(17.0)), // dry softwood (pyrolysis ~573 K)
    ws(1750.0, 1250.0, 0.4, 600.0, None, Some(5.0)),        // AP/Al/HTPB composite propellant
    ws(1650.0, 1130.0, 0.26, 354.0, None, Some(4.6)),       // TNT-class high explosive
];

pub fn wall_si(mat: u32) -> Option<&'static WallSi> {
    WALL_SI.get(mat as usize)
}

pub fn matter_info(id: u32) -> Option<&'static MatterInfo> {
    MATTER_INFO.iter().find(|m| m.id == id)
}

pub fn matter_name(id: u32) -> &'static str {
    MATTER.iter().find(|e| e.tool == Tool::Matter(id)).map(|e| e.name).unwrap_or("Matter")
}

pub fn all() -> impl Iterator<Item = &'static Element> {
    ELEMENTS.iter().chain(MATTER.iter()).chain(EXTRA.iter())
}

pub fn find(tool: Tool) -> Option<&'static Element> {
    all().find(|e| e.tool == tool)
}
