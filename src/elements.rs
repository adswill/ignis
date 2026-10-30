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
    MatInfo { name: "Steel", color: Color32::from_rgb(128, 134, 142), strength: 4.0, t_soft: 1.7, t_melt: 5.0, cond: 1.0, cap: 12.0, shock: None },
    MatInfo { name: "Aluminium", color: Color32::from_rgb(194, 199, 209), strength: 2.0, t_soft: 1.0, t_melt: 2.2, cond: 2.6, cap: 9.0, shock: None },
    MatInfo { name: "Copper", color: Color32::from_rgb(191, 110, 69), strength: 1.5, t_soft: 1.6, t_melt: 3.6, cond: 3.6, cap: 11.0, shock: None },
    MatInfo { name: "Titanium", color: Color32::from_rgb(112, 128, 168), strength: 6.5, t_soft: 2.0, t_melt: 5.6, cond: 0.5, cap: 10.0, shock: None },
    MatInfo { name: "Inconel", color: Color32::from_rgb(62, 84, 92), strength: 5.0, t_soft: 3.3, t_melt: 5.4, cond: 0.35, cap: 14.0, shock: None },
    MatInfo { name: "Ceramic", color: Color32::from_rgb(224, 214, 194), strength: 1.2, t_soft: 7.0, t_melt: 9.5, cond: 0.12, cap: 16.0, shock: Some(2.2) },
    MatInfo { name: "Cooled copper", color: Color32::from_rgb(168, 84, 70), strength: 3.0, t_soft: 1.6, t_melt: 3.6, cond: 3.6, cap: 11.0, shock: None },
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
pub const MATTER: [Element; 100] = [
    // ---- powders ----
    mt(10, "SAND", "Sand", Cat::Powders, (209, 178, 112), "Inert. Piles up; smothers fires and tamps charges. Melts into glass above ~1700 K.", "Sand is mostly quartz (SiO2). Glass makers melt it at about 1700 °C; lightning striking a beach makes glass tubes called fulgurites.", "powder inert silica glass"),
    mt(11, "CHAR", "Charcoal", Cat::Powders, (69, 48, 33), "Fuel. Glows and burns in air above ~700 K. A cloud of it burns explosively.", "Charcoal is nearly pure carbon: wood heated without air until everything else has cooked off.", "carbon fuel black powder"),
    mt(12, "SULF", "Sulfur", Cat::Powders, (245, 219, 31), "Fuel. Ignites at only ~505 K, so it helps other mixes catch.", "Sulfur lowers black powder's ignition temperature and makes it burn faster. It burns with a blue flame into choking SO2.", "brimstone black powder fuel"),
    mt(13, "KNO3", "Saltpeter", Cat::Powders, (194, 217, 252), "Oxidiser: releases O2 above ~673 K. With charcoal (+ sulfur) = black powder; with sugar = rocket candy; with iron filings = sparks.", "Black powder is 75% saltpeter, 15% charcoal and 10% sulfur, a recipe unchanged for 700 years.", "potassium nitrate oxidiser black powder rocket candy"),
    mt(14, "SUGR", "Sugar", Cat::Powders, (250, 235, 194), "Fuel. With saltpeter it makes rocket candy. Sugar dust in air can explode.", "Rocket candy (KNO3 + sugar) has powered amateur rockets past 10 km. In 2008 a sugar-dust explosion destroyed a refinery in Georgia, USA.", "sucrose fuel rocket candy dust"),
    mt(64, "FLOR", "Flour", Cat::Powders, (242, 227, 194), "Harmless in a heap, but a dust cloud of it burns explosively. Fine dust hangs in the air.", "Grain-silo and flour-mill dust explosions have levelled buildings since the 1700s; a 1878 Minneapolis mill blast killed 18 people.", "dust explosion wheat grain silo powder fuel"),
    mt(37, "COAL", "Coal", Cat::Powders, (13, 13, 15), "Slow, hot, smoky fuel (ignites ~720 K). Coal dust is explosive.", "Coal-dust explosions were the great killer in mines: the 1906 Courrières disaster in France killed 1099 miners.", "fuel carbon smoke dust mine"),
    mt(16, "NH4P", "Ammonium perchlorate", Cat::Powders, (194, 199, 245), "Oxidiser. With aluminium or magnesium powder or sugar: composite rocket propellant that burns faster under pressure.", "APCP propellant is about 70% ammonium perchlorate by mass.", "ap oxidiser apcp rocket"),
    mt(47, "KCLO", "Potassium perchlorate", Cat::Powders, (209, 242, 224), "Strong oxidiser. With aluminium or magnesium powder = flash powder: explodes even in the open.", "Flash powder (KClO4 + Al) makes the bang in firecrackers and salutes; it burns in about a millisecond.", "kclo4 flash powder oxidiser firework"),
    mt(60, "KMNO", "Potassium permanganate", Cat::Chemicals, (92, 13, 107), "Purple oxidiser. With glycerin it bursts into flame by itself after a few seconds. Catalyses peroxide.", "Permanganate + glycerin is the classic 'fire without a match' demo; survival kits carry permanganate for this.", "kmno4 oxidiser glycerin purple"),
    mt(17, "RUST", "Rust", Cat::Powders, (173, 71, 26), "Iron oxide. With aluminium (or magnesium) powder = thermite: hard to light (~1350 K) but makes molten iron. Catalyses peroxide.", "Thermite burns at about 2500 °C and is used to weld railway tracks together.", "iron oxide thermite"),
    mt(59, "CUO", "Copper oxide", Cat::Powders, (20, 43, 76), "Black oxide. With aluminium powder = copper thermite: far faster than iron thermite, sprays molten copper.", "Copper thermite is so fast it is practically explosive; it is used in some fireworks and military igniters.", "cuo thermite oxide copper"),
    mt(18, "ANIT", "Ammonium nitrate", Cat::Powders, (247, 224, 214), "Oxidiser. With diesel, kerosene or charcoal = ANFO, which only smoulders unless a strong shock sets it off.", "ANFO is the world's most used mining explosive. It needs a booster charge to detonate.", "anfo fertiliser oxidiser"),
    mt(57, "GUNP", "Gunpowder", Cat::Powders, (56, 61, 76), "Ready-mixed black powder. Lights from a flame (~700 K) and burns fast with lots of smoke; confined, it bursts.", "Gunpowder was invented in 9th-century China by alchemists looking for an elixir of life.", "black powder premix firework"),
    mt(48, "NCEL", "Nitrocellulose", Cat::Powders, (235, 224, 163), "Smokeless powder: self-oxidising, lights at only ~440 K and burns very fast and clean.", "Guncotton was discovered in 1846 when Christian Schönbein wiped up spilled acid with his wife's cotton apron, which then flashed away.", "smokeless powder guncotton propellant"),
    mt(28, "SALT", "Salt", Cat::Chemicals, (240, 242, 247), "Dissolves in water into brine. Melts ice (salt water freezes only at 252 K).", "Roads are salted because brine stays liquid down to about -21 °C.", "nacl brine sodium chloride"),
    mt(46, "MNO2", "Manganese dioxide", Cat::Chemicals, (61, 43, 76), "Catalyst: makes hydrogen peroxide decompose violently into steam and oxygen. Not used up.", "MnO2 + concentrated peroxide is the 'elephant toothpaste' demo, and the catalyst in early peroxide rocket motors.", "catalyst peroxide elephant toothpaste"),
    mt(58, "DICE", "Dry ice", Cat::Powders, (214, 235, 255), "Solid CO2 at 195 K. Sublimates into heavy cold CO2 gas that smothers fires.", "Dry ice skips the liquid state at normal pressure; CO2 fire extinguishers spray a snow of it.", "co2 carbon dioxide extinguisher cold"),
    mt(27, "SNOW", "Snow", Cat::Powders, (219, 237, 255), "Fluffy ice crystals. Drifts slowly down, melts into water at 273 K.", "Fresh snow is about 90% air, which is why it insulates so well.", "ice cold frozen"),
    mt(25, "ASH", "Ash", Cat::Powders, (158, 158, 153), "Inert residue left after burning.", "Wood ash is mostly calcium carbonate and potassium salts: potash.", "slag residue"),
    // ---- metals ----
    mt(15, "ALPW", "Aluminium powder", Cat::Metals, (140, 158, 189), "Metal fuel that burns at ~3000 K. With rust = thermite; with perchlorate = APCP or flash powder. Fizzes in acid and lye.", "The Space Shuttle boosters' propellant was 16% aluminium powder.", "aluminum metal fuel thermite apcp"),
    mt(39, "MG", "Magnesium", Cat::Metals, (204, 212, 219), "Lights at ~780 K and burns blinding white at 3100 K. Water and CO2 make it worse. Fizzes in acid.", "Magnesium flares and old camera flashbulbs burn magnesium; a burning Mg fire splits water into hydrogen.", "metal flare white thermite ribbon"),
    mt(40, "FE", "Iron filings", Cat::Metals, (66, 56, 51), "Rusts when wet (faster in salt water). Burns as sparks when hot; with saltpeter makes sparklers.", "Steel wool burns in air with orange sparks; sparkler sparks are burning iron filings.", "iron metal rust sparkler"),
    mt(41, "NA", "Sodium", Cat::Metals, (219, 212, 168), "Floats on water and reacts: hydrogen + heat + lye. Burns with a yellow flame.", "Sodium is soft enough to cut with a knife and is stored under oil because it reacts with moisture in the air.", "alkali metal water hydrogen"),
    mt(42, "K", "Potassium", Cat::Metals, (163, 163, 214), "Like sodium but more violent: lights its own hydrogen and spits burning bits.", "Potassium hits water so fast that the metal surface explodes into a spray before the hydrogen even ignites (a Coulomb explosion).", "alkali metal water hydrogen lilac"),
    // ---- liquids ----
    mt(19, "WATR", "Water", Cat::Liquids, (41, 128, 250), "Boils at 373 K and soaks up huge amounts of heat; freezes at 273 K. Touching molten metal or burning oil it flashes into a steam explosion.", "Boiling 1 kg of water absorbs 2.3 MJ, about as much as heating it from 0 to 100 °C five times.", "h2o extinguish liquid"),
    mt(29, "BRIN", "Brine", Cat::Liquids, (56, 178, 158), "Salt water: freezes at 252 K, boils dry into salt crystals, rusts iron fast.", "Sea water is about 3.5% salt; the Dead Sea is ten times saltier.", "salt water sea"),
    mt(20, "KERO", "Kerosene", Cat::Liquids, (235, 143, 36), "Fuel. Floats on water; its vapour burns as a pool fire.", "Kerosene (RP-1) fuelled the Saturn V's first stage and the Falcon 9.", "rp1 jet fuel oil liquid anfo"),
    mt(31, "GASO", "Gasoline", Cat::Liquids, (247, 235, 133), "Very volatile: evaporates at room temperature into a flammable vapour cloud that flashes back to the pool.", "Gasoline's flash point is about -43 °C: there is always enough vapour over it to burn.", "petrol fuel volatile vapour"),
    mt(32, "DESL", "Diesel", Cat::Liquids, (143, 117, 31), "Heavy fuel: hard to light (flash point ~325 K) but burns long and smoky. With ammonium nitrate = ANFO.", "Diesel engines need no spark: compressing air heats it enough to ignite the injected fuel.", "fuel oil anfo smoky"),
    mt(33, "OIL", "Cooking oil", Cat::Liquids, (235, 168, 15), "Heats past 600 K and its vapour catches fire. Water on burning oil flashes to steam and throws flaming oil everywhere.", "Grease fires cause most kitchen fires; firefighters' demos of a cup of water on a burning pan make a 3-metre fireball.", "grease fire fat kitchen"),
    mt(34, "NPLM", "Napalm", Cat::Liquids, (214, 87, 20), "Sticky gelled fuel: barely flows, sticks to things and burns long, hot and sooty.", "Napalm is gasoline thickened with a gelling agent so it sticks and burns for minutes instead of seconds.", "gel fuel sticky incendiary"),
    mt(21, "ETOH", "Ethanol", Cat::Liquids, (128, 224, 235), "Evaporates easily and burns with a clean, almost invisible flame.", "The V-2 rocket burned 75% ethanol with liquid oxygen.", "alcohol fuel liquid"),
    mt(61, "GLYC", "Glycerin", Cat::Liquids, (230, 214, 184), "Thick, sweet liquid. With potassium permanganate it ignites on its own.", "Glycerin plus nitric acid makes nitroglycerin; on its own it is used in soap and food.", "glycerol permanganate"),
    mt(22, "LOX", "Liquid oxygen", Cat::Liquids, (107, 168, 255), "90 K. With any fuel it is violently explosive.", "LOX soaked into charcoal (oxyliquit) was once used as a mining explosive.", "lox oxygen cryogenic oxyliquit"),
    mt(30, "LN2", "Liquid nitrogen", Cat::Liquids, (204, 230, 245), "77 K. Boils into nitrogen and freezes water it touches.", "Liquid nitrogen boils at -196 °C; a litre makes almost 700 litres of gas.", "cryogenic cold freeze nitrogen"),
    mt(23, "NITR", "Nitroglycerin", Cat::Liquids, (204, 230, 92), "Detonates from heat (~490 K) or a shock. Handle with care.", "Alfred Nobel invented dynamite to tame nitroglycerin by soaking it into clay.", "ng explosive dynamite liquid"),
    mt(55, "LAVA", "Lava", Cat::Liquids, (242, 66, 13), "1300 K molten rock: flows slowly, sets things alight, cools into stone. Steam explosions in water.", "Basalt lava erupts at about 1100-1200 °C; Hawaiian lava entering the sea builds new land in clouds of steam.", "magma volcano molten rock"),
    mt(24, "MFE", "Molten iron", Cat::Liquids, (255, 122, 20), "2800 K liquid metal: flows, sets things alight and solidifies into steel. Water touching it explodes into steam.", "Iron melts at 1811 K; blast furnaces tap it out at around 1800 K.", "lava metal thermite"),
    mt(54, "MGLS", "Molten glass", Cat::Liquids, (255, 153, 51), "Glowing, very viscous; cools into glass below ~1350 K.", "Glassblowers work glass at about 1100 °C, when it is like thick honey.", "lava glass molten"),
    mt(36, "MWAX", "Molten wax", Cat::Liquids, (252, 240, 194), "Liquid wax: its vapour burns (that is how a candle works). Freezes back below 324 K.", "A candle flame burns wax vapour: the wick draws up molten wax that the flame boils.", "candle paraffin"),
    mt(63, "MPLS", "Molten plastic", Cat::Liquids, (61, 204, 168), "Drips and burns with thick black smoke.", "Burning plastic drips spread fires downward; its smoke is dense with soot.", "melted plastic smoke"),
    // ---- chemicals (liquids) ----
    mt(44, "HCL", "Hydrochloric acid", Cat::Chemicals, (153, 235, 66), "Eats metals (magnesium fast, aluminium, iron slowly) giving hydrogen; dissolves rust; with lye gives hot salt water.", "Your stomach makes dilute hydrochloric acid, about pH 1.5-3.5.", "acid hcl muriatic hydrogen"),
    mt(43, "LYE", "Lye", Cat::Chemicals, (184, 224, 173), "Sodium hydroxide solution. Dissolves aluminium (hydrogen); neutralises acids with heat.", "Drain cleaner is lye plus aluminium bits: the hydrogen fizz and heat stir up the clog.", "naoh base alkali caustic"),
    mt(45, "H2O2", "Hydrogen peroxide", Cat::Chemicals, (158, 204, 255), "90% HTP. On a catalyst (manganese dioxide, permanganate, rust) it decomposes into 1000 K steam + oxygen: a monopropellant.", "The Bell rocket belt and the Me 163 rocket fighter flew on hydrogen peroxide.", "htp peroxide monopropellant oxidiser"),
    // ---- solids ----
    mt(26, "ICE", "Ice", Cat::Structure, (153, 217, 252), "Frozen water at 255 K: melts at 273 K (salt melts it colder).", "Ice is about 9% less dense than water, which is why lakes freeze from the top down.", "frozen water cold"),
    mt(53, "GLAS", "Glass", Cat::Structure, (178, 224, 230), "Transparent, inert, melts above ~1450 K. Good for lab beakers.", "Glass is an amorphous solid: its atoms are as disordered as a liquid's, frozen in place.", "beaker window silica"),
    mt(56, "STNE", "Stone", Cat::Structure, (107, 107, 115), "Basalt rock: does not burn, melts into lava above ~1230 K.", "Most of the ocean floor is basalt, made from lava that cooled underwater.", "rock basalt"),
    mt(35, "WAX", "Wax", Cat::Structure, (245, 230, 168), "Paraffin: melts at ~330 K into a liquid whose vapour burns. Makes heat-triggered plugs.", "Paraffin wax melts at 46-68 °C and releases 42 MJ/kg when burned, about as much as diesel.", "candle paraffin plug"),
    mt(62, "PLST", "Plastic", Cat::Structure, (31, 158, 143), "Polyethylene: melts at ~430 K, then drips and burns very smokily.", "Polyethylene is the world's most common plastic, basically a solid very long-chain paraffin.", "polyethylene pe melt"),
    mt(38, "PAPR", "Paper", Cat::Burnable, (237, 219, 184), "Lights at 451 °F (506 K) and burns fast, leaving ash.", "Ray Bradbury's 'Fahrenheit 451' is named after paper's auto-ignition temperature.", "cardboard fuel tinder"),
    mt(65, "FUSE", "Fuse", Cat::Burnable, (143, 102, 61), "Black-match cord: burns steadily along its length and lights whatever is at the end.", "Bickford's safety fuse (1831) burns at a steady ~1 cm/s, which cut mining deaths dramatically.", "wick timer black match"),
    mt(49, "TNT", "TNT", Cat::Burnable, (224, 189, 102), "Insensitive high explosive: a flame only makes it burn (very sooty). Needs a strong shock (> 25 atm) to detonate.", "TNT is so insensitive it was first used as a yellow dye; it was 30 years before anyone realised it was an explosive.", "trinitrotoluene high explosive"),
    mt(50, "C4", "C4", Cat::Burnable, (199, 199, 173), "Plastic explosive. Burns quietly if lit; detonates only from a blasting cap or another blast (> 30 atm).", "Soldiers in Vietnam lit small pieces of C4 to heat rations: it just burns unless shocked.", "rdx plastic explosive"),
    mt(51, "DYNA", "Dynamite", Cat::Burnable, (204, 51, 36), "Nitroglycerin in clay: safer to handle, but detonates from a blast (> 7 atm) or when a fire heats it.", "Nobel's dynamite (1867) made blasting tunnels like the Gotthard possible and funded the Nobel prizes.", "nitroglycerin stick explosive"),
    mt(52, "CAP", "Blasting cap", Cat::Burnable, (204, 128, 71), "Small, very sensitive primary explosive (lead azide): any flame, heat or bump sets it off. Use it to detonate TNT, C4 and ANFO.", "Primary explosives like lead azide detonate when merely lit, so tiny amounts are used to start big secondary charges.", "detonator primer booster"),
    // ---- propellant ingredients, fuels and oxidisers (ids 66-84) ----
    mt(66, "HTPB", "HTPB binder", Cat::Chemicals, (143, 97, 76), "Hydroxyl-terminated polybutadiene: the rubbery binder of composite propellant. Cast, it cures into a tough grain: slows the burn a little, makes the grain crack-resistant.", "The Space Shuttle boosters burned 16 % HTPB-type binder with 70 % ammonium perchlorate and 16 % aluminium.", "binder rubber polymer apcp"),
    mt(67, "EPOX", "Epoxy", Cat::Chemicals, (219, 143, 71), "Epoxy resin binder: glues oxidiser and fuel into a hard, tough grain; mixed into candy it slows the burn and resists cracking.", "Amateur rocketeers cast KNO3/sorbitol with a little epoxy for tougher, slower grains.", "binder resin glue grain"),
    mt(68, "CuCr", "Copper chromite", Cat::Chemicals, (20, 43, 28), "Burn-rate catalyst: about 1 % copper chromite raises the burning rate of composite propellant by 50-100 %.", "Adding a few percent of iron oxide or copper chromite to APCP is the standard way to speed it up.", "catalyst burn rate accelerator"),
    mt(69, "KClO3", "Potassium chlorate", Cat::Powders, (247, 242, 173), "Oxidiser. Mixtures with sugar or red phosphorus are friction sensitive: stirring them can ignite them.", "Strike-anywhere matches and early fireworks used chlorate: many a pharmacist was blown up grinding it.", "oxidiser friction sensitive chlorate"),
    mt(70, "NaNO3", "Sodium nitrate", Cat::Powders, (204, 194, 168), "Oxidiser, like saltpeter but slower and hygroscopic. Burns with a yellow flame.", "Chile saltpetre was mined by the million tonnes for fertiliser and gunpowder.", "chile saltpetre oxidiser yellow"),
    mt(71, "Ti", "Titanium powder", Cat::Metals, (76, 97, 133), "Metal fuel: ignites ~870 K, sheds brilliant white sparks.", "Titanium sponge sparks are used in fireworks 'gold-sparkle' effects and in flares.", "metal fuel sparks"),
    mt(72, "Zn", "Zinc powder", Cat::Metals, (133, 163, 178), "Metal fuel: burns with a pale blue-green flame and thick white zinc oxide smoke.", "Zinc + sulfur ('Cosmic Rocket') was a 1930s toy propellant, and zinc dust is a classic smoke mix.", "metal fuel smoke"),
    mt(73, "P", "Red phosphorus", Cat::Powders, (143, 13, 41), "Fuel that ignites easily; with chlorate it is shock and friction sensitive.", "Safety-match strikers contain red phosphorus; the heads contain potassium chlorate.", "phosphorus fuel match"),
    mt(74, "SORB", "Sorbitol", Cat::Powders, (247, 219, 230), "Sugar alcohol fuel: KNO3 + sorbitol (KNSB) is a popular rocket propellant, easy to cast, slower at low pressure.", "Sorbitol is also a sweetener in sugar-free sweets.", "knsb fuel sugar alcohol rocket"),
    mt(75, "DEXT", "Dextrose", Cat::Powders, (252, 245, 173), "Sugar fuel: KNO3 + dextrose (KNDX) is the fastest of the sugar rocket candies.", "Dextrose is glucose, the sugar your cells burn for energy.", "glucose fuel rocket candy knDX"),
    mt(76, "RUBR", "Rubber", Cat::Burnable, (33, 33, 36), "Static solid: burns slowly with thick black smoke and a sooty flame.", "Burning tyres can smoulder for months and release oily smoke.", "tyre fuel smoke"),
    mt(77, "COTN", "Cotton", Cat::Burnable, (245, 247, 255), "Cellulose fibre: catches fast and burns quickly (flash fire). Soaked in nitric/sulfuric acid it becomes guncotton.", "Cotton wool burns so quickly it is used as tinder.", "cellulose tinder fibre"),
    mt(78, "CDST", "Coal dust", Cat::Powders, (61, 64, 79), "Fine coal: a hazard in mines. A suspended cloud with a spark explodes.", "Coal dust explosions killed over 1000 miners at Courrieres in 1906.", "coal dust explosion mine"),
    mt(79, "SAWD", "Sawdust", Cat::Powders, (219, 168, 97), "Wood dust: burns slowly in a pile, explosively as a cloud.", "Sawmill and furniture-factory dust explosions are a leading industrial hazard.", "wood dust fuel"),
    mt(80, "ACET", "Acetone", Cat::Liquids, (143, 240, 194), "Very volatile solvent: flash point -20 degC, flammable vapour fills the air above it.", "Acetone is also made by your body when it burns fat.", "solvent nail polish volatile"),
    mt(81, "MEOH", "Methanol", Cat::Liquids, (189, 178, 245), "Alcohol fuel: burns with an almost invisible pale blue flame. Toxic.", "Methanol-burning dragsters and Indy cars burn it because it can be put out with water.", "alcohol fuel racing"),
    mt(82, "HNO3", "Nitric acid", Cat::Chemicals, (250, 204, 71), "White fuming nitric acid: strong oxidiser and acid. Hypergolic with hydrazine: they ignite on contact.", "Early rockets such as the Wasserfall and the Corporal burned nitric acid with aniline-type fuels that ignited on contact.", "oxidiser acid hypergolic"),
    mt(83, "N2H4", "Hydrazine", Cat::Liquids, (235, 204, 219), "Monopropellant and fuel: hypergolic with nitric acid or N2O4 (ignites on contact). Decomposes on a catalyst.", "Hydrazine thrusters are on most satellites and the Space Shuttle's auxiliary power units.", "monopropellant hypergolic satellite"),
    mt(84, "N2O4", "Dinitrogen tetroxide", Cat::Liquids, (158, 31, 15), "Storable oxidiser (boils at 294 K, so it is shown chilled). Hypergolic with hydrazine: the Apollo service module and Titan II fuel pair.", "The Apollo lunar module landed on a hydrazine/N2O4 hypergolic engine: it could not fail to light.", "hypergolic oxidiser apollo nto"),
    // ---- nozzle, liner and insulation materials (ids 85-91) ----
    mt(85, "W", "Tungsten", Cat::Structure, (87, 92, 112), "Highest-melting metal (3695 K): throat inserts of solid rocket nozzles. Heavy and a superb heat sink.", "Tungsten throat inserts survive 3000 K exhaust for the whole burn of a solid booster.", "nozzle throat insert refractory"),
    mt(86, "GRPH", "Graphite", Cat::Structure, (87, 89, 102), "Throat and nozzle material: conducts heat well, never melts, but oxidises/erodes slowly in a hot oxidising flow.", "The Shuttle boosters' nozzle throats were carbon-phenolic and graphite; they ablated a few mm per launch.", "nozzle throat carbon"),
    mt(87, "ABLT", "Phenolic ablative", Cat::Structure, (138, 76, 36), "Liner: when heated it chars and its pyrolysis gas carries heat away. The best insulator you can paint on a case.", "Apollo's heat shield was an ablative: it was meant to burn away.", "liner heatshield insulation char"),
    mt(88, "PCHR", "Phenolic char", Cat::Structure, (61, 28, 18), "What is left of an ablative: porous carbon that erodes in a hot fast flow (throat erosion).", "The char layer is what actually protects the structure behind it during re-entry.", "carbon char liner"),
    mt(89, "FGLS", "Fibreglass", Cat::Structure, (204, 224, 148), "Glass-fibre composite: light, insulating, softens above ~1100 K.", "Rocket motor cases for model rockets are filament-wound fibreglass.", "composite insulation case"),
    mt(90, "FBRK", "Firebrick", Cat::Structure, (224, 184, 107), "Refractory brick: insulates a furnace or engine up to ~1900 K.", "Firebrick lines furnaces and the Saturn V's flame trench.", "refractory insulation furnace"),
    mt(91, "AERO", "Aerogel", Cat::Structure, (148, 189, 247), "Silica aerogel: 99 % air, the best solid insulator (k about 0.015 W/mK).", "A flower on a slab of aerogel does not wilt over a Bunsen flame.", "insulation foam silica"),
    // ---- engine, cryogenic and construction materials (ids 92-109) ----
    mt(92, "LH2", "Liquid hydrogen", Cat::Liquids, (207, 230, 255), "Cryogenic fuel at 20 K. It boils off violently and the hydrogen burns with an almost invisible flame.", "The Space Shuttle main engines burned liquid hydrogen with liquid oxygen; the tank held 1.5 million litres at -253 C.", "cryogenic fuel hydrogen liquid rocket"),
    mt(93, "LCH4", "Liquid methane", Cat::Liquids, (180, 215, 240), "Cryogenic fuel at 111 K: boils to methane gas, which burns with a blue-yellow flame. Used in new reusable rockets.", "Methane is denser than hydrogen and does not soak into tank walls, which makes reusable engines easier.", "cryogenic fuel methane lng rocket"),
    mt(94, "LPG", "Liquid propane", Cat::Liquids, (230, 200, 120), "Liquefied petroleum gas: stored under pressure, boils at -42 C and burns with a clean yellow flame.", "Propane is the fuel in bottled-gas camping stoves and barbecues.", "lpg propane fuel liquid gas"),
    mt(95, "IPA", "Isopropanol", Cat::Liquids, (215, 235, 235), "Rubbing alcohol: evaporates fast and burns with a pale flame.", "Isopropanol burns clean enough that it is used as a fuel for small alcohol burners and hand-sanitiser fires.", "alcohol solvent fuel flammable liquid"),
    mt(96, "LIME", "Limestone", Cat::Structure, (214, 208, 190), "Calcium carbonate rock. Above ~1100 K it calcines: it releases CO2 and leaves lime.", "Cement is made by heating limestone to 1450 C in a kiln.", "rock carbonate kiln cement calcine"),
    mt(97, "CAO", "Quicklime", Cat::Powders, (235, 232, 222), "Calcium oxide, the residue of burnt limestone. Inert here; very hot lime glows brightly (limelight).", "Limelights lit theatre stages in the 1800s: a gas flame heating a block of lime to dazzling white.", "powder lime inert limelight kiln"),
    mt(98, "SODA", "Baking soda", Cat::Powders, (245, 245, 248), "Sodium bicarbonate. Above ~430 K it decomposes into CO2 and steam: a powder that smothers flames.", "Dry-chemical extinguishers use sodium bicarbonate: heat releases CO2 right at the fire.", "powder extinguisher co2 smother chemical"),
    mt(99, "GYP", "Gypsum plaster", Cat::Structure, (236, 230, 214), "Plasterboard core: it holds water of crystallisation and steams at ~450 K, soaking up heat. A fire barrier.", "Drywall protects buildings in a fire because the gypsum cooks off its water before the studs behind it get hot.", "wall board plaster fire barrier"),
    mt(100, "CONC", "Concrete", Cat::Structure, (150, 150, 152), "Cement, sand and gravel. Dense, strong in compression and a poor conductor; spalls and melts only in extreme heat.", "Concrete lines launch flame trenches because it survives rocket exhaust long enough for a short burn.", "building flame trench dense masonry"),
    mt(101, "SOIL", "Soil", Cat::Powders, (110, 80, 52), "Loose earth. Heaps up, smothers fires and absorbs heat; wet soil would hold water (not modelled).", "A shovel of dirt is the oldest firefighting tool there is.", "powder earth dirt smother"),
    mt(102, "EPS", "Polystyrene foam", Cat::Structure, (250, 250, 250), "Expanded foam: very light and a good insulator. Softens near 400 K and melts to plastic, burning with a thick black smoke.", "Packing foam is 98 % air; its fires produce sooty smoke because styrene is a carbon-rich fuel.", "foam plastic insulation packing smoke"),
    mt(103, "B", "Boron powder", Cat::Metals, (70, 50, 40), "Amorphous boron: ignites near 1000 K and burns with a green-white glow. A high-energy fuel for ramjets and fireworks.", "Boron was investigated as a ramjet fuel because it releases more heat per volume than any hydrocarbon.", "metal powder fuel green firework"),
    mt(104, "ZR", "Zirconium powder", Cat::Metals, (155, 160, 170), "Burns at over 4000 K with a blinding white light. Used in camera flashbulbs and as a rocket fuel additive.", "Early flash bulbs were glass globes filled with zirconium foil and oxygen.", "metal powder flash white bright"),
    mt(105, "CFRP", "Carbon fibre", Cat::Structure, (40, 42, 48), "Woven carbon-fibre composite: light, very stiff and a poor conductor across the weave. Burns away slowly in a hot oxidising flow.", "Rocket motor cases and nose cones are wound from carbon fibre because it is stiffer than steel at a fifth of the weight.", "composite case light strong"),
    mt(106, "BRAS", "Brass", Cat::Structure, (205, 170, 70), "Copper-zinc alloy: machines well and conducts heat. Melts near 1200 K.", "Brass was used for bullet casings and valve bodies because it resists corrosion and machines cleanly.", "metal alloy conductor casing"),
    mt(107, "BITU", "Bitumen", Cat::Liquids, (35, 30, 28), "Asphalt tar: a very thick black liquid that creeps. Heated, it gives off a thick sooty fuel vapour.", "Roof tar and road asphalt both are bitumen; tar fires burn with heavy black smoke.", "tar asphalt viscous oil smoke"),
    mt(108, "GLYC", "Coolant (glycol)", Cat::Liquids, (120, 220, 120), "Ethylene glycol engine coolant: boils at 470 K, much hotter than water, and does not burn easily.", "Car coolant is half glycol, half water; it lowers the freezing point and raises the boiling point.", "antifreeze engine coolant liquid"),
    mt(109, "OIL", "Engine oil", Cat::Liquids, (160, 120, 40), "Lubricating oil: viscous, burns with a sooty flame once hot enough.", "Oil carries heat away from engine bearings; an oil fire is hard to put out because water sinks beneath it.", "lubricant mineral oil viscous fuel smoke"),
];

/// Recipes shown on the help card: (name, ingredients, what happens). Ingredient names match element names.
pub const RECIPES: [(&str, &str, &str); 28] = [
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
    ("KNSB", "Saltpeter + sorbitol (65:35), cast", "Easy-to-cast sugar propellant: a steady, smoky burn for seconds"),
    ("Hypergolic storables", "Hydrazine + dinitrogen tetroxide (or nitric acid)", "Ignites on contact, no igniter needed: the Apollo / Titan pair"),
    ("Armstrong's mixture", "Potassium chlorate + red phosphorus", "Friction sensitive: stirring it can set it off"),
    ("Ablative nozzle", "Phenolic liner + tungsten throat insert", "The liner chars and cools the case; tungsten survives the throat's heat"),
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

pub const MATTER_INFO: [MatterInfo; 100] = [
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
    mi(66, 2, 0.93, None, None, Some(640.0), None, "Flammable polymer", "Sooty products").si(1900.0, Some(42.0), None, None, None),
    mi(67, 2, 1.15, None, None, Some(700.0), None, "Flammable resin", "Sooty products").si(1700.0, Some(30.0), None, None, None),
    mi(68, 1, 4.5, None, None, None, None, "Toxic (chromium)", "-").si(700.0, None, None, None, None),
    mi(69, 1, 2.32, Some(641.0), Some(670.0), None, None, "Strong oxidiser; friction sensitive with fuels", "Oxygen when hot").si(1000.0, None, Some(0.4), Some(100.0), None),
    mi(70, 1, 2.26, Some(581.0), Some(720.0), None, None, "Oxidiser", "Oxygen when hot").si(1090.0, None, Some(0.3), Some(180.0), None),
    mi(71, 1, 4.5, Some(1941.0), None, Some(870.0), None, "Burns with white sparks; dust explodes", "Titanium dioxide smoke").si(520.0, Some(19.7), None, Some(295.0), None),
    mi(72, 1, 7.1, Some(693.0), Some(1180.0), Some(740.0), None, "Burns blue-green; dust explodes", "Zinc oxide smoke").si(390.0, Some(5.3), None, Some(112.0), None),
    mi(73, 1, 2.34, None, None, Some(530.0), None, "Flammable; shock sensitive with chlorate", "Phosphorus oxides").si(700.0, Some(24.0), None, None, None),
    mi(74, 1, 1.49, Some(369.0), None, Some(540.0), None, "Flammable; dust explodes", "CO2 + steam").si(1300.0, Some(16.7), None, Some(150.0), None),
    mi(75, 1, 1.54, Some(420.0), None, Some(560.0), None, "Flammable; dust explodes", "CO2 + steam").si(1250.0, Some(15.6), None, Some(200.0), None),
    mi(76, 0, 1.1, None, None, Some(600.0), None, "Burns with thick black smoke", "Soot").si(2000.0, Some(35.0), None, None, None),
    mi(77, 0, 1.5, None, None, Some(500.0), None, "Flash fire", "CO2 + steam").si(1300.0, Some(16.0), None, None, None),
    mi(78, 1, 1.3, None, None, Some(720.0), None, "Dust cloud explodes", "CO2 + heat").si(1300.0, Some(30.0), None, None, None),
    mi(79, 1, 0.25, None, None, Some(550.0), None, "Dust cloud explodes", "CO2 + steam").si(1700.0, Some(17.0), None, None, None),
    mi(80, 2, 0.79, Some(178.0), Some(329.0), Some(738.0), None, "Highly flammable vapour", "Fuel vapour").si(2150.0, Some(29.0), None, Some(98.0), Some(501.0)),
    mi(81, 2, 0.79, Some(176.0), Some(338.0), Some(737.0), None, "Flammable, toxic, near-invisible flame", "Fuel vapour").si(2530.0, Some(19.9), None, Some(99.0), Some(1100.0)),
    mi(82, 2, 1.51, Some(231.0), Some(356.0), None, None, "Strong oxidiser and acid; hypergolic with hydrazine", "NOx fumes").si(1750.0, None, None, Some(43.0), Some(480.0)),
    mi(83, 2, 1.01, Some(275.0), Some(387.0), Some(510.0), None, "Toxic, hypergolic with nitric acid and N2O4", "Hot gas").si(3080.0, Some(19.4), Some(3.1), Some(395.0), Some(1230.0)),
    mi(84, 2, 1.44, Some(262.0), Some(294.0), None, None, "Toxic oxidiser; hypergolic with hydrazine", "NO2 / O2").si(1560.0, None, None, Some(157.0), Some(413.0)),
    mi(85, 0, 19.3, Some(3695.0), None, None, None, "Inert, very heavy", "-").si(132.0, None, None, Some(192.0), None),
    mi(86, 0, 1.8, None, None, Some(1000.0), None, "Oxidises above ~1000 K in air", "CO2").si(710.0, Some(32.8), None, None, None),
    mi(87, 0, 1.45, None, None, Some(750.0), None, "Chars; its pyrolysis gas burns", "Pyrolysis gas").si(1400.0, Some(25.0), None, None, None),
    mi(88, 0, 1.2, None, None, Some(1000.0), None, "Erodes in a hot flow", "CO / CO2").si(1000.0, Some(30.0), None, None, None),
    mi(89, 0, 1.8, Some(1400.0), None, None, None, "Softens above ~1100 K", "Molten glass").si(840.0, None, None, None, None),
    mi(90, 0, 2.0, Some(1900.0), None, None, None, "Inert", "-").si(1000.0, None, None, None, None),
    mi(91, 0, 0.1, Some(1700.0), None, None, None, "Inert; shatters under shock", "-").si(840.0, None, None, None, None),
    mi(92, 2, 0.071, None, Some(20.0), None, None, "Cryogenic; boils off to flammable gas", "Hydrogen gas (fuel)").si(9700.0, Some(120.0), None, None, Some(446.0)),
    mi(93, 2, 0.42, Some(91.0), Some(111.0), None, None, "Cryogenic; boils off to flammable gas", "Methane gas (fuel)").si(3500.0, Some(50.0), None, None, Some(510.0)),
    mi(94, 2, 0.58, Some(85.0), Some(231.0), None, None, "Boils off to flammable heavy gas", "Propane gas (fuel)").si(2500.0, Some(46.0), None, None, Some(426.0)),
    mi(95, 2, 0.79, Some(184.0), Some(356.0), Some(672.0), None, "Flammable liquid", "CO2 + steam").si(2600.0, Some(30.5), None, None, Some(663.0)),
    mi(96, 0, 2.7, Some(1600.0), Some(1100.0), None, None, "Inert; releases CO2 when very hot", "CO2").si(880.0, None, None, None, None),
    mi(97, 1, 3.3, Some(2900.0), None, None, None, "Inert", "-").si(750.0, None, None, None, None),
    mi(98, 1, 2.2, None, Some(430.0), None, None, "Inert; gives off CO2 and steam when hot", "CO2 + steam").si(890.0, None, None, None, None),
    mi(99, 0, 2.3, Some(1700.0), Some(450.0), None, None, "Inert; steams when hot", "Steam").si(1090.0, None, None, None, None),
    mi(100, 0, 2.4, Some(1500.0), None, None, None, "Inert", "-").si(880.0, None, None, None, None),
    mi(101, 1, 1.5, None, None, None, None, "Inert", "-").si(800.0, None, None, None, None),
    mi(102, 0, 0.05, Some(450.0), None, None, None, "Melts; burns smokily", "Molten plastic").si(1300.0, Some(40.0), None, None, None),
    mi(103, 1, 2.3, Some(2350.0), None, Some(1000.0), None, "Burns very hot; dust explodes", "Boron oxide smoke").si(1100.0, Some(58.0), None, None, None),
    mi(104, 1, 6.5, Some(2128.0), None, Some(840.0), None, "Burns extremely bright; dust explodes", "Zirconia smoke").si(270.0, Some(12.0), None, None, None),
    mi(105, 0, 1.6, Some(3800.0), None, None, None, "Inert", "-").si(800.0, None, None, None, None),
    mi(106, 0, 8.5, Some(1180.0), None, None, None, "Inert; melts", "Molten metal").si(380.0, None, None, Some(160.0), None),
    mi(107, 2, 1.05, None, Some(600.0), Some(720.0), None, "Viscous; burns smokily when hot", "Soot + CO2").si(920.0, Some(40.0), None, None, Some(300.0)),
    mi(108, 2, 1.11, Some(260.0), Some(470.0), None, None, "Toxic; steams when hot", "Steam-like vapour").si(2400.0, None, None, None, Some(800.0)),
    mi(109, 2, 0.87, None, Some(750.0), Some(520.0), None, "Flammable when hot", "Soot + CO2").si(1900.0, Some(42.0), None, None, Some(250.0)),
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
