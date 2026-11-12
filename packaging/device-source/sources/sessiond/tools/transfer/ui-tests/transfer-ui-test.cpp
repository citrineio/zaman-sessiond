#include <QGuiApplication>
#include <QQmlEngine>
#include <QQmlComponent>
#include <QQuickItem>
#include <QQuickWindow>
#include <QTimer>
#include <QEventLoop>
#include <QVariant>
#include <QDebug>
#include <QColor>
#include <QJSValue>
#include <memory>

static void settle(int milliseconds=220) {
    QEventLoop loop;
    QTimer::singleShot(milliseconds, &loop, &QEventLoop::quit);
    loop.exec();
}

static QQuickItem* findItem(QQuickItem* root, const QString& name) {
    if (root->objectName()==name) return root;
    for (auto child:root->childItems())
        if (auto found=findItem(child,name)) return found;
    return nullptr;
}

static bool checkProgress(QQuickItem* root, const QString& name, double expected) {
    auto bar=findItem(root,name);
    if (!bar || bar->width()<=0 || bar->height()<=0 || bar->childItems().isEmpty()) {
        qWarning()<<"Missing rendered progress bar"<<name; return false;
    }
    const double value=bar->property("value").toDouble();
    const double rendered=bar->childItems().first()->width()/bar->width();
    if (qAbs(value-expected)>.000001 || qAbs(rendered-expected)>.001) {
        qWarning()<<"Progress mismatch"<<name<<value<<rendered<<expected; return false;
    }
    return true;
}

static bool checkRemoval(QQuickItem* root,const QString& name,const char* payloadProperty,const QString& labelProperty) {
    settle(60);
    auto row=findItem(root,name);
    if (!row || !row->isVisible() || !row->property("retiring").toBool() || row->property("dismissOffset").toDouble()<=0 || row->opacity()>=1) {
        qWarning()<<"Missing sliding removal"<<name;return false;
    }
    auto payload=row->property(payloadProperty);
    if (payload.metaType()==QMetaType::fromType<QJSValue>()) payload=payload.value<QJSValue>().toVariant();
    if (payload.toMap()[labelProperty].toString().isEmpty()) {
        qWarning()<<"Removed card lost its text"<<name;return false;
    }
    settle();
    if (findItem(root,name)) {qWarning()<<"Removed card remains after transition"<<name;return false;}
    return true;
}

int main(int argc, char** argv) {
    QGuiApplication app(argc, argv);
    if (argc != 2) return 2;
    QQmlEngine engine;
    bool warnings = false;
    QObject::connect(&engine,&QQmlEngine::warnings,[&](const QList<QQmlError>& errors) {
        warnings=true;
        for (const auto& error:errors) qWarning()<<error;
    });
    const QVariantMap job{{"id",QString(32,'a')},{"batch","test"},{"name","Controller test.iso"},
        {"phase","needs-destination"},{"candidates",QVariantList{"ps1","ps2"}},
        {"size",double(4294967313LL)},{"received",double(4294967313LL)},
        {"created",double(1)},{"error",""},{"evidence","Choose a collection"}};
    const QVariantMap snapshot{{"jobs",QVariantList{job}}, {"enabled",true},
        {"pin","123456"},{"alias","Zaman"},{"seconds",600},
        {"systems",QVariantList{QVariantMap{{"id","ps1"},{"name","PlayStation"}},
                               QVariantMap{{"id","ps2"},{"name","PlayStation 2"}}}}};
    QQuickWindow window;
    window.resize(900,1080);
    window.show();
    for (const QString name: {QString("TransferPage.qml"),QString("NotificationsPage.qml"),QString("ScrapePage.qml")}) {
        QQmlComponent component(&engine,QUrl::fromLocalFile(QString(argv[1])+"/"+name));
        if (component.isError()) { qWarning()<<component.errors(); return 1; }
        QVariantMap props;
        if (name=="TransferPage.qml") props={{"snapshot",snapshot},{"available",true}};
        else if (name=="NotificationsPage.qml") props={{"imports",QVariantList{job}}};
        else {
            const QVariantMap prompt{{"id","prompt"},{"kind","match"},{"question","Which game is this?"},
                {"choices",QVariantList{QVariantMap{{"label","No"},{"value","n"}},QVariantMap{{"label","Yes"},{"value","y"}}}}};
            const QVariantMap scrapeFixture{{"id",QString(32,'b')},{"folders",QVariantList{"ps2"}},
                {"phase","complete"},{"stage","Choose a game match"},{"progress",.5},
                {"results",QVariantList{}},{"log","Preparing metadata"},{"prompt",prompt}};
            const QVariantMap scrapeSnapshot{{"available",true},{"folders",QVariantList{
                QVariantMap{{"id","ps2"},{"name","PlayStation 2"},{"platform","ps2"},{"supported",true}}}},
                {"jobs",QVariantList{scrapeFixture}}};
            props={{"available",true},{"snapshot",scrapeSnapshot}};
        }
        std::unique_ptr<QObject> object(component.createWithInitialProperties(props));
        auto item=qobject_cast<QQuickItem*>(object.get());
        if (!item) { qWarning()<<component.errors(); return 1; }
        item->setWidth(760); item->setHeight(900);
        item->setParentItem(window.contentItem());
        settle();
        if (name=="TransferPage.qml") {
            const QString barName="transferProgress-"+QString(32,'a');
            for (double fraction:{.25,.75}) {
                auto updatedJob=job;
                updatedJob["phase"]="receiving";
                updatedJob["received"]=job["size"].toDouble()*fraction;
                auto updatedSnapshot=snapshot;
                updatedSnapshot["jobs"]=QVariantList{updatedJob};
                item->setProperty("snapshot",updatedSnapshot);
                settle();
                if (!checkProgress(item,barName,fraction)) return 1;
            }
            auto failedJob=job;
            failedJob["phase"]="failed";
            failedJob["size"]=double(4284329308LL);
            failedJob["received"]=double(4283957248LL);
            auto failedSnapshot=snapshot;
            failedSnapshot["jobs"]=QVariantList{failedJob};
            item->setProperty("snapshot",failedSnapshot);
            settle();
            if (!checkProgress(item,barName,4283957248.0/4284329308.0)) return 1;
            item->setProperty("snapshot",snapshot);
            settle();
            if (findItem(item,barName)->isVisible()) return 1;
            if (!QMetaObject::invokeMethod(item,"move",Q_ARG(QVariant,QVariant(1)))) return 1;
            if (!QMetaObject::invokeMethod(item,"activate")) return 1;
            settle();
            if (item->property("detailId").toString()!=QString(32,'a')) return 1;
            if (findItem(item,"transferDetailProgress")->isVisible()) return 1;
            for (const QString phase:{QString("needs-scrape"),QString("scraping"),QString("ready"),QString("complete")}) {
                auto stageJob=job;stageJob["phase"]=phase;
                auto stageSnapshot=snapshot;stageSnapshot["jobs"]=QVariantList{stageJob};
                item->setProperty("snapshot",stageSnapshot);
                settle();
                auto bar=findItem(item,"transferDetailProgress");
                const bool working=phase=="scraping" || phase=="needs-scrape";
                if (!bar || bar->isVisible()!=working || bar->property("indeterminate").toBool()!=working) return 1;
                if (working) {
                    if (bar->property("value").toDouble()!=0 || bar->childItems().first()->width()>=bar->width()*.5) return 1;
                    const double before=bar->childItems().first()->x();settle();
                    if (qAbs(bar->childItems().first()->x()-before)<1) return 1;
                }
            }
            if (!QMetaObject::invokeMethod(item,"back")) return 1;
            for (const QString phase:{QString("receiving"),QString("scraping"),QString("ready"),QString("complete"),QString("receiving")}) {
                auto stageJob=job;stageJob["phase"]=phase;
                auto stageSnapshot=snapshot;stageSnapshot["jobs"]=QVariantList{stageJob};
                item->setProperty("snapshot",stageSnapshot);settle();
                auto bar=findItem(item,barName);
                auto label=findItem(item,"transferStage-"+QString(32,'a'));
                if (!bar || !label) return 1;
                const QString text=label->property("text").toString();
                if (phase!="receiving" && text.contains("100%")) return 1;
                if (phase=="scraping" && (!bar->property("indeterminate").toBool() || bar->property("value").toDouble()!=0)) return 1;
                if ((phase=="ready" || phase=="complete") && bar->isVisible()) return 1;
                if (phase=="ready") {
                    auto hint=findItem(item,"transferHint-"+QString(32,'a'));
                    if (!hint || !hint->property("text").toString().contains("Close this menu to update your library")) return 1;
                }
                if (phase=="receiving") {
                    if (!checkProgress(item,barName,1) || qAbs(bar->childItems().first()->x())>.001) return 1;
                }
            }
            auto historyJob=job;
            historyJob["phase"]="complete";
            auto historySnapshot=snapshot;
            historySnapshot["jobs"]=QVariantList{historyJob};
            historySnapshot["historyCount"]=1;
            item->setProperty("snapshot",historySnapshot);
            item->setProperty("detailId",QString());
            item->setProperty("selectedId",QString(32,'a'));
            if (!item->property("canDismiss").toBool() || !item->property("canClear").toBool()) return 1;
            if (!QMetaObject::invokeMethod(item,"clear")) return 1;
            settle();
            if (item->property("detailId").toString()!="history") return 1;
            if (item->property("canDismiss").toBool() || item->property("canClear").toBool()) return 1;
            if (!QMetaObject::invokeMethod(item,"back")) return 1;
            settle();
            auto empty=snapshot;empty["jobs"]=QVariantList{};empty["historyCount"]=0;
            item->setProperty("snapshot",empty);
            if (!checkRemoval(item,"transfer-row:"+QString(32,'a'),"payload","name")) return 1;
            if (item->property("selectedId").toString()!="receiver") return 1;
        } else if (name=="NotificationsPage.qml") {
            if (!item->property("hasItems").toBool()) return 1;
            item->setProperty("imports",QVariantList{});
            if (!checkRemoval(item,"notification-row:transfer:"+QString(32,'a'),"payload","title")) return 1;
        } else {
            auto primary=findItem(item,"artworkPrimaryAction");
            if (!primary || primary->property("text").toString()!="Find artwork & info") return 1;
            auto action=findItem(item,"artworkSurface-start");
            auto collection=findItem(item,"artworkSurface-collection");
            auto options=findItem(item,"artworkSurface-options");
            if (!action || !collection || !options || qAbs(collection->height()-options->height())>.001) return 1;
            if (action->property("color").value<QColor>()!=QColor("#EAC28D")) return 1;
            const double largeHeight=action->height();
            item->setProperty("selected",2);settle();
            if (action->property("color").value<QColor>()==QColor("#EAC28D") || options->property("color").value<QColor>()!=QColor("#EAC28D")) return 1;
            if (qAbs(action->height()-largeHeight)>.001 || largeHeight<=options->height()) return 1;
            item->setProperty("selected",1);
            if (item->property("selected").toInt()!=1 || item->property("canDismiss").toBool()) return 1;
            item->setProperty("selected",2);
            if (!QMetaObject::invokeMethod(item,"activate")) return 1;
            settle();
            if (item->property("view").toString()!="options") return 1;
            if (!QMetaObject::invokeMethod(item,"back")) return 1;
            item->setProperty("selected",0);
            if (!QMetaObject::invokeMethod(item,"activate")) return 1;
            settle();
            if (item->property("view").toString()!="folders") return 1;
            if (!QMetaObject::invokeMethod(item,"back")) return 1;
            if (!QMetaObject::invokeMethod(item,"open",Q_ARG(QVariant,QVariant("")),Q_ARG(QVariant,QVariant("")),Q_ARG(QVariant,QVariant(QString(32,'b'))))) return 1;
            settle();
            if (item->property("view").toString()!="job") return 1;
            if (!checkProgress(item,"artworkProgress",.5)) return 1;
            auto scrapeSnapshot=item->property("snapshot").toMap();
            auto scrapeJobs=scrapeSnapshot["jobs"].toList();
            auto scrapeJob=scrapeJobs[0].toMap();
            item->setProperty("selected",1);
            for (int poll=0;poll<4;++poll) {
                scrapeJob["log"]=QString("Status poll %1").arg(poll);
                scrapeSnapshot["jobs"]=QVariantList{scrapeJob};
                item->setProperty("snapshot",scrapeSnapshot);
                settle();
                if (item->property("selected").toInt()!=1) {
                    qWarning()<<"Polling reset the selected scraping answer";return 1;
                }
            }
            scrapeJob["prompt"]=QVariantMap{{"id","text-prompt"},{"question","Enter a response?"},{"text",true},{"choices",QVariantList{}}};
            scrapeSnapshot["jobs"]=QVariantList{scrapeJob};
            item->setProperty("snapshot",scrapeSnapshot);
            settle();
            if (item->property("selected").toInt()!=0) return 1;
            if (!QMetaObject::invokeMethod(item,"activate")) return 1;
            settle();
            if (!item->property("editingText").toBool()) return 1;
            if (!QMetaObject::invokeMethod(item,"back")) return 1;
            if (!QMetaObject::invokeMethod(item,"back")) return 1;
            item->setProperty("selected",3);
            if (!QMetaObject::invokeMethod(item,"activate")) return 1;
            settle();
            if (item->property("view").toString()!="activity") return 1;
            scrapeJob["prompt"]=QVariant();
            scrapeSnapshot["jobs"]=QVariantList{scrapeJob};item->setProperty("snapshot",scrapeSnapshot);settle();
            item->setProperty("selected",0);
            scrapeSnapshot["jobs"]=QVariantList{};item->setProperty("snapshot",scrapeSnapshot);
            if (!checkRemoval(item,"artwork-row:activity:"+QString(32,'b'),"modelData","label")) return 1;
            if (item->property("view").toString()!="activity" || item->property("selected").toInt()!=0) return 1;
        }
        if (window.grabWindow().isNull()) { qWarning()<<"No rendered frame"; return 1; }
        item->setParentItem(nullptr);
    }
    QQmlComponent iconComponent(&engine,QUrl::fromLocalFile(QString(argv[1])+"/MenuIcon.qml"));
    std::unique_ptr<QObject> icon(iconComponent.createWithInitialProperties({{"name","scrape"}}));
    auto iconItem=qobject_cast<QQuickItem*>(icon.get());
    if (!iconItem) {qWarning()<<iconComponent.errors();return 1;}
    iconItem->setParentItem(window.contentItem());iconItem->setWidth(40);iconItem->setHeight(40);settle();
    if (window.grabWindow().isNull()) return 1;
    iconItem->setProperty("name","notifications");settle();
    iconItem->setParentItem(nullptr);
    return warnings ? 1 : 0;
}
