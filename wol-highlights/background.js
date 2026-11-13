const ext = typeof browser !== "undefined" ? browser : chrome;

ext.action.onClicked.addListener(async () => {
  const pageUrl = ext.runtime.getURL("popup.html");
  try {
    const tabs = await ext.tabs.query({});
    const existingTab = tabs.find((t) => t.url === pageUrl);

    if (existingTab && existingTab.id !== undefined) {
      await ext.tabs.update(existingTab.id, { active: true });
      if (existingTab.windowId !== undefined) {
        await ext.windows.update(existingTab.windowId, { focused: true });
      }
    } else {
      await ext.tabs.create({ url: pageUrl });
    }
  } catch (err) {
    console.error("[WOL Highlights Background] Failed to open/focus manager tab:", err);
    ext.tabs.create({ url: pageUrl });
  }
});
