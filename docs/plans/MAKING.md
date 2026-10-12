# Plan: making things (food, tools, buildings)

Status: proposed 2026-10-12. The owner asked for dynamic recipes, and reworked building and tool
making with "lots of dynamics". One system covers all three: making something from materials with
properties, by a method, discovered by trying and passed on by watching.

## One recipe table

Recipes live with the materials (`sim-world`), as data: what goes in (materials and counts), the
method, what comes out, and how long it takes. Methods are physical situations, not menus:

| Method | Needs |
|---|---|
| By hand | nothing |
| With an edge | a blade carried |
| At a fire | a burning fire beside you |
| On a rack | a drying rack beside you, outside winter |
| Digging | a digging stick or bare hands, open ground |

Today's crafts (blade, spear, basket, roast, hide) become rows of this table, so nothing special
is left in code.

## Discovering recipes

Nobody starts knowing what nobody has made. A person learns a recipe by:

- **watching** someone make it (as crafts are learned today), or
- **trying**: someone curious, with spare materials and a method at hand and nothing pressing,
  tries a combination it hasn't tried. Physics decides: if the table has that row, it works and the
  person knows it; if not, the materials are wasted and it remembers not to try that again
  (a bounded set of tried combinations per person). Cooking and crafting talent make tries more
  likely to be sensible (they try combinations closer to ones they know).

What a made thing is good for (it keeps, it warms, it cuts) is then learned like any material:
from using it. Things get compound names from their ingredients (D-131), so bands that discover
different things end up with different words, food and tools.

## Food (M1)

New foods with properties:

| Food | From | Method | Keeps | Feeds |
|---|---|---|---|---|
| Dried meat | meat | on a rack, or at a fire (smoked) | a season or more | a little less than fresh |
| Dried berries | berries | on a rack | a season | a little less than fresh |
| Pemmican | dried meat + dried berries | by hand | a year | more than both |
| Stew | meat + berries | at a fire, with a container | a few days | most per unit |

The drying rack is a new structure (wood). Preserved food is what makes stocking a hut for winter
work instead of rotting there.

## Tools (M2)

Tools are put together from parts, each with properties:

- **parts:** a head (blade, sharpened stone), a shaft or handle (wood), a binding (cord, twisted
  from hide strips or plant fibre);
- **tools:** axe (head + handle + cord: chops much faster), spear (head + shaft + cord: strikes
  surer than a sharpened stick), digging stick (sowing and burying faster), container (basket
  lined with hide: holds water and stew), cloak (hide + cord: warmer than a loose hide);
- **wear:** each tool has a durability; use wears it, it breaks, and it's mended (the same recipe
  with fewer parts) or remade. Better makers make tools that last longer.

## Buildings (M3)

Structures get designs made of materials, each with properties: how warm, how long it lasts, how
many it shelters, how long it takes:

| Design | Materials | Notes |
|---|---|---|
| Lean-to | a little wood | quick, cool, falls apart soon |
| Hut | wood, hides for the roof | today's hut; warm |
| Stone hut | stone and wood | slow to build, very warm, lasts for generations |
| Drying rack | wood | for drying food |
| Storage pit | digging | food keeps three times longer |
| Windbreak | wood or stone | warms a camp or a hearth a little |

Structures wear with weather and need **repair** (owners and family mend them with the same
materials); a hut can be **upgraded** (lean-to to hut, hut to stone hut). Designs are discovered
by trying and spread by watching, like recipes. Building talent sets speed and how long it lasts.

## Phases

| Phase | What |
|---|---|
| M0 | The recipe table; today's crafts moved into it; trying and watching to learn recipes |
| M1 | Preserved and mixed food, the drying rack |
| M2 | Parts, composite tools, wear and mending |
| M3 | Building designs, wear and repair, upgrades, storage pits, windbreaks |

Each is measured over 10 years in 20 valleys and kept on even if it costs lives (the owner prefers
a harder, more interesting world), with the numbers recorded. The aim: less food rotting, people
who specialise in what they make well, and bands whose tools, food and homes differ.
