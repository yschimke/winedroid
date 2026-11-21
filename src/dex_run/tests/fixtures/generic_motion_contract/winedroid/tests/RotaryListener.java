package winedroid.tests;

import android.view.MotionEvent;
import android.view.View;

public class RotaryListener implements View.OnGenericMotionListener {
    @Override
    public boolean onGenericMotion(View view, MotionEvent event) {
        RotaryState.listenerHits++;
        return true;
    }
}
