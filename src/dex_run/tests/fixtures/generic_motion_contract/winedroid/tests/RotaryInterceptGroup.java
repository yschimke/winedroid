package winedroid.tests;

import android.content.Context;
import android.view.MotionEvent;
import android.widget.FrameLayout;

public class RotaryInterceptGroup extends FrameLayout {
    public RotaryInterceptGroup(Context context) {
        super(context);
    }

    @Override
    public boolean dispatchGenericMotionEvent(MotionEvent event) {
        RotaryState.interceptHits++;
        return super.dispatchGenericMotionEvent(event);
    }

    @Override
    public boolean onGenericMotionEvent(MotionEvent event) {
        RotaryState.groupFallbackHits++;
        return true;
    }
}
