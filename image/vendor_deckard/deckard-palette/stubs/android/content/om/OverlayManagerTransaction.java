package android.content.om;

public class OverlayManagerTransaction {
    public static final class Builder {
        public Builder registerFabricatedOverlay(FabricatedOverlay overlay) {
            throw new RuntimeException("stub");
        }

        public Builder unregisterFabricatedOverlay(OverlayIdentifier overlay) {
            throw new RuntimeException("stub");
        }

        public Builder setEnabled(OverlayIdentifier overlay, boolean enable, int userId) {
            throw new RuntimeException("stub");
        }

        public OverlayManagerTransaction build() {
            throw new RuntimeException("stub");
        }
    }
}
