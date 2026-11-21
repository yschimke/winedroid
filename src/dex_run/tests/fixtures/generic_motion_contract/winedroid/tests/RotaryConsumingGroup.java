package winedroid.tests;

import android.content.Context;
import android.view.MotionEvent;
import android.widget.FrameLayout;

public class RotaryConsumingGroup extends FrameLayout {
    public RotaryConsumingGroup(Context context) {
        super(context);
    }

    @Override
    public boolean dispatchGenericMotionEvent(MotionEvent event) {
        RotaryState.consumingHits++;
        return true;
    }
}
