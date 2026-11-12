.pragma library

function array(model) {
    if (!model) return [];
    if (model.toVarArray) return model.toVarArray();
    var out = [];
    for (var i = 0; i < model.count; ++i) out.push(model.get(i));
    return out;
}
function collection(game) {
    return game && game.collections && game.collections.count ? game.collections.get(0).name : "";
}
function identity(game) {
    if (!game) return "";
    if (game.files && game.files.count) return String(game.files.get(0).path);
    return collection(game) + "::" + game.title;
}
function asset(game, names) {
    if (!game || !game.assets) return "";
    for (var i = 0; i < names.length; ++i) {
        if (game.assets[names[i]]) return game.assets[names[i]];
    }
    return "";
}
function titleOrder(a, b) {
    return String(a.sortBy || a.title).toLocaleLowerCase().localeCompare(String(b.sortBy || b.title).toLocaleLowerCase());
}
function timestamp(value) {
    var n = new Date(value).getTime();
    return isNaN(n) ? 0 : n;
}
function recent(games) {
    return games.filter(function(g) { return g.playCount > 0; }).sort(function(a, b) {
        return timestamp(b.lastPlayed) - timestamp(a.lastPlayed) || titleOrder(a, b);
    });
}
function filter(games, collectionName, favoritesOnly, query, sort) {
    var needle = query.trim().toLocaleLowerCase();
    var result = games.filter(function(g) {
        if (favoritesOnly && !g.favorite) return false;
        if (needle && String(g.title).toLocaleLowerCase().indexOf(needle) < 0) return false;
        if (!collectionName) return true;
        for (var i = 0; g.collections && i < g.collections.count; ++i) {
            if (g.collections.get(i).name === collectionName) return true;
        }
        return false;
    });
    result.sort(sort === 1 ? function(a, b) {
        return timestamp(b.lastPlayed) - timestamp(a.lastPlayed) || titleOrder(a, b);
    } : titleOrder);
    return result;
}
function metadata(game) {
    if (!game) return "";
    var parts = [];
    if (game.releaseYear > 0) parts.push(String(game.releaseYear));
    if (game.genre) parts.push(game.genre);
    if (game.players > 0) parts.push(game.players === 1 ? "1 player" : "Up to " + game.players + " players");
    return parts.join("  /  ");
}
function duration(seconds) {
    var minutes = Math.floor((seconds || 0) / 60);
    return minutes < 60 ? minutes + " min" : Math.floor(minutes / 60) + " h " + (minutes % 60) + " min";
}

// Preserve identity artwork; crop only scene artwork. URLs are compared as
// strings here, never by comparing a QML QUrl to a JavaScript string.
function artwork(game, roles) {
    var result = [], seen = {};
    if (!game || !game.assets) return result;
    for (var i = 0; i < roles.length; ++i) {
        var role = roles[i], url = String(game.assets[role] || "");
        if (!url || seen[url]) continue;
        seen[url] = true;
        result.push({url: url, fit: ["logo", "boxFront", "poster", "tile"].indexOf(role) >= 0});
    }
    return result;
}
