REPO_ROOT := $(shell pwd)
DEB_OUT   := $(REPO_ROOT)/target/debian
DBNET_SRC := $(REPO_ROOT)/assets/stabrise-text_detection_dbnet_ml_v02_model.onnx
DBNET_PKG := $(REPO_ROOT)/packaging/lenzu-models-dbnet
NOTICES   := $(REPO_ROOT)/lenzu/NOTICES.crates.md

.PHONY: deb deb-client deb-hud deb-dbnet notices clean-deb install uninstall

deb: deb-client deb-hud deb-dbnet
	@echo
	@echo "Built .debs in $(DEB_OUT):"
	@ls -1 $(DEB_OUT)/*.deb 2>/dev/null

# Auto-generate the transitive Rust-crate license list from Cargo.lock.
# Output is consumed by the in-app About dialog and shipped in the .deb.
notices: $(NOTICES)

$(NOTICES): lenzu/Cargo.toml lenzu/about.toml lenzu/about.hbs Cargo.lock
	@command -v cargo-about >/dev/null || cargo install cargo-about --features cli
	cd lenzu && cargo about generate about.hbs -o NOTICES.crates.md
	@echo "Regenerated $(NOTICES)"

deb-client: notices
	@command -v cargo-deb >/dev/null || cargo install cargo-deb
	cargo deb -p lenzu --output $(DEB_OUT)

deb-hud:
	cd lenzu_server && npm run deb
	@mkdir -p $(DEB_OUT)
	@cp lenzu_server/dist-deb/*.deb $(DEB_OUT)/

# Hand-rolled .deb for the AGPL-3.0 DBNet model.
# Staged into a build dir so the source tree stays clean.
deb-dbnet:
	@test -f $(DBNET_SRC) || { echo "ERROR: $(DBNET_SRC) not found (LFS pulled?)"; exit 1; }
	@mkdir -p $(DEB_OUT)
	@rm -rf $(DEB_OUT)/lenzu-models-dbnet
	@mkdir -p $(DEB_OUT)/lenzu-models-dbnet/DEBIAN
	@mkdir -p $(DEB_OUT)/lenzu-models-dbnet/usr/share/lenzu/models
	@mkdir -p $(DEB_OUT)/lenzu-models-dbnet/usr/share/doc/lenzu-models-dbnet
	cp $(DBNET_PKG)/DEBIAN/control  $(DEB_OUT)/lenzu-models-dbnet/DEBIAN/
	cp $(DBNET_PKG)/DEBIAN/postinst $(DEB_OUT)/lenzu-models-dbnet/DEBIAN/
	chmod 755 $(DEB_OUT)/lenzu-models-dbnet/DEBIAN/postinst
	cp $(DBNET_PKG)/copyright $(DEB_OUT)/lenzu-models-dbnet/usr/share/doc/lenzu-models-dbnet/
	cp $(DBNET_SRC) $(DEB_OUT)/lenzu-models-dbnet/usr/share/lenzu/models/
	dpkg-deb --build --root-owner-group $(DEB_OUT)/lenzu-models-dbnet $(DEB_OUT)/lenzu-models-dbnet_0.2.0_all.deb

clean-deb:
	rm -rf $(DEB_OUT) lenzu_server/dist-deb $(NOTICES)

install: deb
	$(REPO_ROOT)/scripts/install-lenzu.sh --with-dbnet

uninstall:
	$(REPO_ROOT)/scripts/uninstall-lenzu.sh
