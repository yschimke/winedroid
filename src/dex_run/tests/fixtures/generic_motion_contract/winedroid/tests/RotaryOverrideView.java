package winedroid.tests;

import android.content.Context;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.View;

public class RotaryOverrideView extends View {
    public RotaryOverrideView(Context context) {
        super(context);
    }

    @Override
    public boolean onGenericMotionEvent(MotionEvent event) {
        if (event.isFromSource(InputDevice.SOURCE_ROTARY_ENCODER)
                && event.getAction() == MotionEvent.ACTION_SCROLL) {
            RotaryState.overrideHits++;
            RotaryState.overrideAxis = event.getAxisValue(MotionEvent.AXIS_SCROLL);
            return true;
        }
        return super.onGenericMotionEvent(event);
    }
}
