#include <QGuiApplication>
#include <QQmlEngine>
#include <QQmlComponent>
#include <QQuickWindow>
#include <QQuickItem>
#include <QEventLoop>
#include <QTimer>
#include <QVariant>
#include <QUrl>
#include <QColor>
#include <QImage>
#include <cstdio>
#include <cstdlib>

static bool qmlError = false;
static void messages(QtMsgType, const QMessageLogContext &, const QString &message)
{
    std::fprintf(stderr, "%s\n", qPrintable(message));
    if (message.contains("ReferenceError") || message.contains("TypeError")
        || message.contains("Binding loop") || message.contains("Cannot assign")
        || message.contains("Unable to assign") || message.contains("Error decoding")) qmlError = true;
}
static void check(bool good, const char *message)
{
    if (!good) { std::fprintf(stderr, "FAIL: %s\n", message); std::exit(1); }
}
static void pump(int milliseconds = 250)
{
    QEventLoop loop; QTimer::singleShot(milliseconds, &loop, &QEventLoop::quit); loop.exec();
}
// ListView delegates live in the visual tree. Their QObject owner may be the
// delegate model rather than their visual parent, so findChild is insufficient.
static QQuickItem *findVisualItem(QQuickItem *root, const QString &name)
{
    if (root->objectName() == name) return root;
    for (auto *child : root->childItems()) {
        if (auto *found = findVisualItem(child, name)) return found;
    }
    return nullptr;
}
static void dumpVisualTree(QQuickItem *root, int depth = 0)
{
    std::fprintf(stderr, "%*s%s name=%s visible=%d size=%.0fx%.0f\n", depth * 2, "",
                 root->metaObject()->className(), qPrintable(root->objectName()),
                 root->isVisible(), root->width(), root->height());
    for (auto *child : root->childItems()) dumpVisualTree(child, depth + 1);
}
static QQuickItem *item(QQuickItem *root, const QString &name)
{
    auto *result = findVisualItem(root, name);
    // Allow pending delegate creation/layout to complete under device load.
    for (int attempt = 0; !result && attempt < 20; ++attempt) {
        pump(50); result = findVisualItem(root, name);
    }
    if (!result) dumpVisualTree(root);
    check(result, qPrintable("missing " + name)); return result;
}
static void checkVisualLookup()
{
    QQuickItem scene;
    QObject delegateOwner;
    auto *delegate = new QQuickItem;
    delegate->setParent(&delegateOwner);
    delegate->setParentItem(&scene);
    auto *label = new QQuickItem(delegate);
    label->setObjectName("delegate-lookup-fixture");
    check(!scene.findChild<QQuickItem *>(label->objectName()), "fixture has independent QObject ownership");
    check(findVisualItem(&scene, label->objectName()) == label, "lookup finds visually parented delegates");
}

int main(int argc, char **argv)
{
    QGuiApplication app(argc, argv);
    qInstallMessageHandler(messages);
    checkVisualLookup();
    QQmlEngine engine;
    QQmlComponent mockComponent(&engine);
    mockComponent.setData(R"QML(import QtQml
QtObject {
    property var records: []
    property bool available: true
    property string error: ""
    function send(message) {}
})QML", QUrl());
    QObject *client = mockComponent.create();
    check(client, "mock notification client compiles"); client->setParent(&engine);
    QQmlComponent component(&engine, QUrl::fromLocalFile(QString(MENU_QML_ROOT) + "/NotificationsPage.qml"));
    QObject *object = component.createWithInitialProperties({{"client", QVariant::fromValue(client)}});
    if (!object) std::fprintf(stderr, "%s\n", qPrintable(component.errorString()));
    auto *page = qobject_cast<QQuickItem *>(object);
    check(page, "actual NotificationsPage compiles");
    QQuickWindow window; window.setColor(QColor("#102030")); window.resize(700, 650);
    page->setParent(&window); page->setParentItem(window.contentItem());
    page->setWidth(650); page->setHeight(500); window.show(); pump();
    check(!page->property("hasItems").toBool(), "empty source has no items");
    check(!item(page, "notificationHistory")->isVisible(), "empty history is hidden");
    check(item(page, "notificationEmpty")->isVisible(), "empty-state message is visible");
    check(page->property("selectedId").toString().isEmpty(), "empty view has no selected transaction");

    QVariantMap terminal{{"id", "completed"}, {"title", "Installed Game"}, {"body", "Ready"},
                         {"source", "Dukkan"}, {"timestamp", 100}, {"category", "install"},
                         {"urgency", 1}, {"dismissed", false}};
    client->setProperty("records", QVariantList{terminal}); pump();
    check(page->property("hasItems").toBool(), "stored notification updates page records");
    check(page->property("selectedId").toString() == "completed", "stored notification updates selection");
    auto *history = item(page, "notificationHistory");
    check(history->isVisible() && history->property("count").toInt() == 1,
          "stored notification creates one visible ListView row");
    auto *title = item(page, "notification-title:completed");
    check(title->isVisible() && title->width() > 0 && title->height() > 0
          && title->property("text").toString() == "Installed Game", "stored notification has visible text");
    const QString id(32, 'a'), rowId = "transaction:" + id;
    QVariantMap job{{"id", id}, {"name", "Game"}, {"operation", "install"},
                    {"phase", "running"}, {"stage", "Installing"}, {"percent", 30}, {"created", 100}};
    page->setProperty("transactions", QVariantList{job}); pump();
    auto *row = item(page, "notification-row:" + rowId);
    auto *progress = item(page, "notification-progress:" + rowId);
    check(progress->isVisible() && progress->property("text").toString().startsWith("30%"), "live progress appears");
    job["percent"] = 70; page->setProperty("transactions", QVariantList{job}); pump();
    check(item(page, "notification-row:" + rowId) == row, "percentage update preserves the delegate");
    check(progress->property("text").toString().startsWith("70%"), "progress updates without reopening");
    client->setProperty("records", QVariantList{}); pump();
    check(item(page, "notificationHistory")->isVisible() && row->isVisible(), "live progress survives history clear");
    page->setProperty("transactions", QVariantList{}); pump();
    check(!item(page, "notificationHistory")->isVisible() && item(page, "notificationEmpty")->isVisible(),
          "last item removal hides the card and restores the empty message");
    check(page->property("selectedId").toString().isEmpty(), "last item removal clears selection");
    // Repeat to catch a stale ListModel role or removal transition.
    client->setProperty("records", QVariantList{terminal}); pump();
    check(item(page, "notification-title:completed")->property("text").toString() == "Installed Game", "repopulated history renders");
    client->setProperty("records", QVariantList{}); pump();
    check(!item(page, "notificationHistory")->isVisible(), "second empty transition has no card");

    QQmlComponent iconComponent(&engine, QUrl::fromLocalFile(QString(MENU_QML_ROOT) + "/MenuIcon.qml"));
    for (const QString &name : {QString("store-dukkan"), QString("store-steam")}) {
        for (const QString &ink : {QString("#DEC09A"), QString("#30271E")}) {
            for (int size : {32, 48}) {
                const bool dukkan = name == "store-dukkan";
                const int iconWidth = dukkan ? size * 5 / 2 : size;
                auto *icon = qobject_cast<QQuickItem *>(iconComponent.createWithInitialProperties({{"name", name}, {"ink", QColor(ink)}}));
                if (!icon) std::fprintf(stderr, "%s\n", qPrintable(iconComponent.errorString()));
                check(icon, "MenuIcon compiles");
                icon->setParent(&window); icon->setParentItem(window.contentItem());
                icon->setY(540); icon->setWidth(iconWidth); icon->setHeight(size); pump();
                if (dukkan) {
                    auto *image = item(icon, "brandImage");
                    check(image->isVisible() && image->property("status").toInt() == 1,
                          "supplied Dukkan sign reaches Image.Ready");
                    check(image->property("paintedWidth").toReal() > image->property("paintedHeight").toReal() * 2,
                          "Dukkan sign retains its wide aspect");
                    check(image->property("mipmap").toBool(), "Dukkan sign uses filtered downsampling");
                } else {
                    auto *steam = item(icon, "steamMark");
                    check(steam->isVisible() && steam->property("paintCount").toInt() > 0,
                          "official Steam vector paths are painted");
                    auto *canvas = item(icon, "steamVectorCanvas");
                    check(canvas->width() >= icon->width() * 2, "Steam renders at twice displayed dimensions");
                    const int paints = steam->property("paintCount").toInt();
                    icon->setWidth(iconWidth + 4); icon->setHeight(size + 4); pump();
                    check(steam->property("paintCount").toInt() > paints, "focus resize repaints the vector geometry");
                    icon->setWidth(iconWidth); icon->setHeight(size); pump();
                }
                const QImage capture = window.grabWindow();
                check(!capture.isNull(), "offscreen rendering returns pixels");
                const qreal ratio = capture.width() / qreal(window.width());
                const QImage region = capture.copy(0, qRound(540 * ratio), qRound(iconWidth * ratio), qRound(size * ratio));
                int painted = 0, solidInk = 0, teal = 0;
                for (int y = 0; y < region.height(); ++y) for (int x = 0; x < region.width(); ++x) {
                    const QColor pixel = region.pixelColor(x, y);
                    if (pixel != window.color()) ++painted;
                    if (pixel == QColor(ink)) ++solidInk;
                    if (pixel.red() < 80 && pixel.green() > 90 && pixel.blue() > 70) ++teal;
                }
                check(painted > 40, "storefront mark renders pixels at menu sizes in each focus state");
                check(dukkan ? teal > 20 : solidInk > 20, "sign retains teal artwork; Steam uses the requested monochrome ink");
                delete icon; pump(50);
            }
        }
    }
    check(!qmlError, "no QML binding/type errors");
    std::puts("PASS: actual Qt empty/repopulate transitions, stored text, stable live progress and supplied/vector storefront marks at two sizes and focus colors.");
}
