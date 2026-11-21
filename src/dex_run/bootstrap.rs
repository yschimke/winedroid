use super::{JAVA_ACC_ABSTRACT, JAVA_ACC_FINAL, JAVA_ACC_INTERFACE, JAVA_ACC_PUBLIC};

pub(super) fn bootstrap_method_return_descriptor(
    class_descriptor: &str,
    method_name: &str,
) -> Option<&'static str> {
    match (class_descriptor, method_name) {
        ("Ljava/lang/Class;", "getRecordComponents") => {
            Some("[Ljava/lang/reflect/RecordComponent;")
        }
        ("Ljava/lang/Class;", "isRecord") => Some("Z"),
        _ => None,
    }
}

pub(super) fn is_bootstrap_class_descriptor(class_descriptor: &str) -> bool {
    class_descriptor == "Ljava/lang/Object;"
        || class_descriptor.starts_with('[')
        || bootstrap_superclass_descriptor(class_descriptor).is_some()
        || is_bootstrap_interface_descriptor(class_descriptor)
        || bootstrap_new_instance_kind(class_descriptor).is_some()
        || matches!(
            class_descriptor,
            "Landroid/os/Build$VERSION;" | "Lsun/misc/Unsafe;"
        )
}

pub(super) fn bootstrap_class_modifiers(class_descriptor: &str) -> u32 {
    if class_descriptor.starts_with('[') || is_primitive_descriptor(class_descriptor) {
        return JAVA_ACC_PUBLIC | JAVA_ACC_FINAL | JAVA_ACC_ABSTRACT;
    }

    if is_bootstrap_interface_descriptor(class_descriptor) {
        return JAVA_ACC_PUBLIC | JAVA_ACC_INTERFACE | JAVA_ACC_ABSTRACT;
    }

    if matches!(
        bootstrap_new_instance_kind(class_descriptor),
        Some(BootstrapNewInstanceKind::Abstract)
    ) {
        return JAVA_ACC_PUBLIC | JAVA_ACC_ABSTRACT;
    }

    JAVA_ACC_PUBLIC
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BootstrapNewInstanceKind {
    Abstract,
    MissingZeroArgumentConstructor,
    PublicZeroArgumentConstructor,
}

pub(super) fn bootstrap_new_instance_kind(
    class_descriptor: &str,
) -> Option<BootstrapNewInstanceKind> {
    if is_bootstrap_interface_descriptor(class_descriptor) {
        return Some(BootstrapNewInstanceKind::Abstract);
    }

    // cite: libcore ClassLoader.java#ClassLoader
    // cite: libcore Enum.java#Enum
    // cite: libcore Number.java#Number
    // cite: libcore AbstractCollection.java#AbstractCollection
    // cite: libcore AbstractList.java#AbstractList
    // cite: libcore AbstractSet.java#AbstractSet
    // cite: libcore AbstractMap.java#AbstractMap
    // cite: libcore AbstractQueue.java#AbstractQueue
    // cite: frameworks-base CharacterStyle.java#CharacterStyle
    // cite: frameworks-base MetricAffectingSpan.java#MetricAffectingSpan
    // cite: frameworks-base ReplacementSpan.java#ReplacementSpan
    // cite: frameworks-base AutofillManager.java#AutofillCallback
    if matches!(
        class_descriptor,
        "Ljava/lang/ClassLoader;"
            | "Ljava/lang/Enum;"
            | "Ljava/lang/Number;"
            | "Ljava/util/AbstractCollection;"
            | "Ljava/util/AbstractList;"
            | "Ljava/util/AbstractSet;"
            | "Ljava/util/AbstractMap;"
            | "Ljava/util/AbstractQueue;"
            | "Landroid/text/style/CharacterStyle;"
            | "Landroid/text/style/MetricAffectingSpan;"
            | "Landroid/text/style/ReplacementSpan;"
            | "Landroid/view/autofill/AutofillManager$AutofillCallback;"
    ) {
        return Some(BootstrapNewInstanceKind::Abstract);
    }

    // cite: libcore Boolean.java#Boolean
    // cite: libcore Byte.java#Byte
    // cite: libcore Character.java#Character
    // cite: libcore Short.java#Short
    // cite: libcore Integer.java#Integer
    // cite: libcore Long.java#Long
    // cite: libcore Float.java#Float
    // cite: libcore Double.java#Double
    if matches!(
        class_descriptor,
        "Ljava/lang/Boolean;"
            | "Ljava/lang/Byte;"
            | "Ljava/lang/Character;"
            | "Ljava/lang/Short;"
            | "Ljava/lang/Integer;"
            | "Ljava/lang/Long;"
            | "Ljava/lang/Float;"
            | "Ljava/lang/Double;"
    ) {
        return Some(BootstrapNewInstanceKind::MissingZeroArgumentConstructor);
    }

    // cite: libcore Object.java#Object
    // cite: libcore String.java#String
    // cite: libcore ArrayList.java#ArrayList
    // cite: libcore ThreadLocal.java#ThreadLocal
    // cite: libcore Date.java#Date
    // cite: libcore Random.java#Random
    // cite: libcore StringWriter.java#StringWriter
    // cite: libcore AtomicBoolean.java#AtomicBoolean
    // cite: libcore AtomicInteger.java#AtomicInteger
    // cite: libcore AtomicReference.java#AtomicReference
    // cite: frameworks-base Rect.java#Rect
    // cite: frameworks-base RectF.java#RectF
    // cite: frameworks-base Matrix.java#Matrix
    // cite: frameworks-base Paint.java#Paint
    // cite: frameworks-base Point.java#Point
    // cite: libcore DefaultHandler.java#DefaultHandler
    if matches!(
        class_descriptor,
        "Ljava/lang/Object;"
            | "Ljava/lang/String;"
            | "Ljava/lang/ThreadLocal;"
            | "Ljava/util/Date;"
            | "Ljava/util/Random;"
            | "Ljava/io/StringWriter;"
            | "Ljava/util/ArrayList;"
            | "Ljava/util/concurrent/atomic/AtomicBoolean;"
            | "Ljava/util/concurrent/atomic/AtomicInteger;"
            | "Ljava/util/concurrent/atomic/AtomicReference;"
            | "Landroid/graphics/Rect;"
            | "Landroid/graphics/RectF;"
            | "Landroid/graphics/Matrix;"
            | "Landroid/graphics/Paint;"
            | "Landroid/graphics/Point;"
            | "Lorg/xml/sax/helpers/DefaultHandler;"
    ) {
        return Some(BootstrapNewInstanceKind::PublicZeroArgumentConstructor);
    }

    None
}

pub(super) fn bootstrap_superclass_descriptor(class_descriptor: &str) -> Option<&'static str> {
    if class_descriptor.starts_with('[') {
        return Some("Ljava/lang/Object;");
    }

    match class_descriptor {
        "Ljava/lang/Object;" => None,
        // cite: frameworks-base AutofillManager.java#AutofillCallback
        "Landroid/view/autofill/AutofillManager$AutofillCallback;" => Some("Ljava/lang/Object;"),
        // cite: libcore Class.java#Class
        // cite: libcore ClassLoader.java#ClassLoader
        // cite: libcore Thread.java#Thread
        // cite: libcore System.java#System
        // cite: libcore Math.java#Math
        "Ljava/lang/Class;"
        | "Ljava/lang/ClassLoader;"
        | "Ljava/lang/Thread;"
        | "Ljava/lang/System;"
        | "Ljava/lang/Math;" => Some("Ljava/lang/Object;"),
        // cite: libcore ThreadLocal.java#ThreadLocal
        // cite: libcore Date.java#Date
        // cite: libcore Random.java#Random
        // cite: libcore AtomicReference.java#AtomicReference
        // cite: frameworks-base Matrix.java#Matrix
        // cite: frameworks-base Paint.java#Paint
        // cite: frameworks-base Point.java#Point
        // cite: frameworks-base Rect.java#Rect
        // cite: frameworks-base RectF.java#RectF
        // cite: libcore DefaultHandler.java#DefaultHandler
        "Ljava/lang/ThreadLocal;"
        | "Ljava/util/Date;"
        | "Ljava/util/Random;"
        | "Ljava/util/concurrent/atomic/AtomicReference;"
        | "Landroid/graphics/Matrix;"
        | "Landroid/graphics/Paint;"
        | "Landroid/graphics/Point;"
        | "Landroid/graphics/Rect;"
        | "Landroid/graphics/RectF;"
        | "Lorg/xml/sax/helpers/DefaultHandler;" => Some("Ljava/lang/Object;"),
        // cite: libcore ByteArrayOutputStream.java#ByteArrayOutputStream
        "Ljava/io/ByteArrayOutputStream;" => Some("Ljava/io/OutputStream;"),
        // cite: libcore OutputStream.java#OutputStream
        "Ljava/io/OutputStream;" => Some("Ljava/lang/Object;"),
        // cite: libcore StringWriter.java#StringWriter
        "Ljava/io/StringWriter;" => Some("Ljava/io/Writer;"),
        // cite: libcore Writer.java#Writer
        "Ljava/io/Writer;" => Some("Ljava/lang/Object;"),
        "Ljava/lang/Throwable;" => Some("Ljava/lang/Object;"),
        "Ljava/lang/Exception;" => Some("Ljava/lang/Throwable;"),
        // cite: libcore ReflectiveOperationException.java#ReflectiveOperationException
        "Ljava/lang/ReflectiveOperationException;" => Some("Ljava/lang/Exception;"),
        // cite: libcore InstantiationException.java#InstantiationException
        "Ljava/lang/InstantiationException;" => Some("Ljava/lang/ReflectiveOperationException;"),
        // cite: libcore IllegalAccessException.java#IllegalAccessException
        "Ljava/lang/IllegalAccessException;" => Some("Ljava/lang/ReflectiveOperationException;"),
        // cite: libcore ClassNotFoundException.java#ClassNotFoundException
        "Ljava/lang/ClassNotFoundException;" => Some("Ljava/lang/ReflectiveOperationException;"),
        "Ljava/lang/RuntimeException;" => Some("Ljava/lang/Exception;"),
        // cite: frameworks-base Resources.java#NotFoundException
        "Landroid/content/res/Resources$NotFoundException;" => Some("Ljava/lang/RuntimeException;"),
        // cite: libcore ClassCastException.java#ClassCastException
        "Ljava/lang/ClassCastException;" => Some("Ljava/lang/RuntimeException;"),
        // cite: libcore NullPointerException.java#NullPointerException
        "Ljava/lang/NullPointerException;" => Some("Ljava/lang/RuntimeException;"),
        "Ljava/lang/IllegalStateException;" => Some("Ljava/lang/RuntimeException;"),
        "Ljava/lang/IllegalArgumentException;" => Some("Ljava/lang/RuntimeException;"),
        // cite: libcore UnsupportedOperationException.java#UnsupportedOperationException
        "Ljava/lang/UnsupportedOperationException;" => Some("Ljava/lang/RuntimeException;"),
        // cite: frameworks-base SQLiteException.java#SQLiteException
        "Landroid/database/sqlite/SQLiteException;" => Some("Landroid/database/SQLException;"),
        // cite: frameworks-base SQLException.java#SQLException
        "Landroid/database/SQLException;" => Some("Ljava/lang/RuntimeException;"),
        // cite: frameworks-base SQLiteCursor.java#SQLiteCursor
        "Landroid/database/sqlite/SQLiteCursor;" => {
            Some("Landroid/database/AbstractWindowedCursor;")
        }
        // cite: frameworks-base AbstractWindowedCursor.java#AbstractWindowedCursor
        "Landroid/database/AbstractWindowedCursor;" => Some("Landroid/database/AbstractCursor;"),
        // cite: frameworks-base AbstractCursor.java#AbstractCursor
        "Landroid/database/AbstractCursor;" => Some("Ljava/lang/Object;"),
        "Ljava/util/concurrent/CancellationException;" => Some("Ljava/lang/IllegalStateException;"),
        // cite: libcore ExecutionException.java#ExecutionException
        "Ljava/util/concurrent/ExecutionException;" => Some("Ljava/lang/Exception;"),
        "Ljava/io/IOException;" => Some("Ljava/lang/Exception;"),
        "Ljava/lang/reflect/Constructor;" => Some("Ljava/lang/reflect/Executable;"),
        "Ljava/lang/reflect/Method;" => Some("Ljava/lang/reflect/Executable;"),
        "Ljava/lang/reflect/Executable;" => Some("Ljava/lang/reflect/AccessibleObject;"),
        "Ljava/lang/reflect/Field;" => Some("Ljava/lang/reflect/AccessibleObject;"),
        "Ljava/lang/reflect/AccessibleObject;" => Some("Ljava/lang/Object;"),
        // cite: libcore WeakReference.java#WeakReference
        "Ljava/lang/ref/WeakReference;" => Some("Ljava/lang/ref/Reference;"),
        // cite: libcore Reference.java#Reference
        "Ljava/lang/ref/Reference;" => Some("Ljava/lang/Object;"),
        "Lorg/json/JSONException;" => Some("Ljava/lang/Exception;"),
        "Ljava/io/FileNotFoundException;" => Some("Ljava/io/IOException;"),
        "Landroid/os/Bundle;" => Some("Landroid/os/BaseBundle;"),
        "Landroid/os/BaseBundle;" => Some("Ljava/lang/Object;"),
        // cite: frameworks-base SystemVibratorManager.java#SystemVibratorManager
        "Landroid/os/SystemVibratorManager;" => Some("Landroid/os/VibratorManager;"),
        "Landroid/app/Activity;" => Some("Landroid/view/ContextThemeWrapper;"),
        "Landroid/app/Application;" => Some("Landroid/content/ContextWrapper;"),
        "Landroid/content/ContextWrapper;" => Some("Landroid/content/Context;"),
        "Landroid/app/ContextImpl;" => Some("Landroid/content/Context;"),
        "Landroid/content/Context;" => Some("Ljava/lang/Object;"),
        // cite: frameworks-base ClipboardManager.java#ClipboardManager
        "Landroid/content/ClipboardManager;" => Some("Landroid/text/ClipboardManager;"),
        // cite: frameworks-base ClipboardManager.java#ClipboardManager
        "Landroid/text/ClipboardManager;" => Some("Ljava/lang/Object;"),
        "Landroid/graphics/RecordingCanvas;" => Some("Landroid/graphics/Canvas;"),
        "Landroid/graphics/Canvas;" => Some("Ljava/lang/Object;"),
        // cite: frameworks-base WindowManagerImpl.java#WindowManagerImpl
        "Landroid/view/WindowManagerImpl;" => Some("Ljava/lang/Object;"),
        // cite: frameworks-base BoringLayout.java#BoringLayout
        "Landroid/text/BoringLayout;" => Some("Landroid/text/Layout;"),
        // cite: frameworks-base Layout.java#Layout
        "Landroid/text/Layout;" => Some("Ljava/lang/Object;"),
        // cite: frameworks-base DecorView.java#DecorView
        "Lcom/android/internal/policy/DecorView;" => Some("Landroid/widget/FrameLayout;"),
        "Landroid/text/style/BackgroundColorSpan;" => Some("Landroid/text/style/CharacterStyle;"),
        "Landroid/text/style/ForegroundColorSpan;" => Some("Landroid/text/style/CharacterStyle;"),
        "Landroid/text/style/CharacterStyle;" => Some("Ljava/lang/Object;"),
        "Landroid/view/ContextThemeWrapper;" => Some("Landroid/content/ContextWrapper;"),
        // cite: frameworks-base MotionEvent.java#MotionEvent
        "Landroid/view/MotionEvent;" => Some("Landroid/view/InputEvent;"),
        // cite: frameworks-base InputEvent.java#InputEvent
        "Landroid/view/InputEvent;" => Some("Ljava/lang/Object;"),
        "Landroid/view/View;" => Some("Ljava/lang/Object;"),
        "Landroid/view/ViewGroup;" => Some("Landroid/view/View;"),
        "Landroid/widget/AdapterView;" => Some("Landroid/view/ViewGroup;"),
        "Landroid/widget/AbsListView;" => Some("Landroid/widget/AdapterView;"),
        "Landroid/widget/ListView;" => Some("Landroid/widget/AbsListView;"),
        "Landroid/widget/FrameLayout;" => Some("Landroid/view/ViewGroup;"),
        "Landroid/widget/LinearLayout;" => Some("Landroid/view/ViewGroup;"),
        "Landroid/widget/RelativeLayout;" => Some("Landroid/view/ViewGroup;"),
        "Landroid/widget/TableLayout;" | "Landroid/widget/TableRow;" => {
            Some("Landroid/widget/LinearLayout;")
        }
        "Landroid/widget/HorizontalScrollView;" | "Landroid/widget/ScrollView;" => {
            Some("Landroid/widget/FrameLayout;")
        }
        "Landroid/widget/TextView;" => Some("Landroid/view/View;"),
        "Landroid/widget/Button;" | "Landroid/widget/EditText;" => {
            Some("Landroid/widget/TextView;")
        }
        "Landroid/widget/ImageView;" | "Landroid/widget/Space;" => Some("Landroid/view/View;"),
        "Landroid/widget/ImageButton;" => Some("Landroid/widget/ImageView;"),
        "Ljava/util/ArrayList;" | "Ljava/util/Arrays$ArrayList;" => {
            Some("Ljava/util/AbstractList;")
        }
        "Ljava/util/AbstractList;" => Some("Ljava/util/AbstractCollection;"),
        "Ljava/util/PriorityQueue;" => Some("Ljava/util/AbstractQueue;"),
        "Ljava/util/AbstractQueue;" => Some("Ljava/util/AbstractCollection;"),
        "Ljava/util/HashMap;"
        | "Ljava/util/IdentityHashMap;"
        | "Ljava/util/TreeMap;"
        | "Ljava/util/WeakHashMap;"
        | "Ljava/util/concurrent/ConcurrentHashMap;" => Some("Ljava/util/AbstractMap;"),
        "Ljava/util/LinkedHashMap;" => Some("Ljava/util/HashMap;"),
        "Ljava/util/HashSet;"
        | "Ljava/util/LinkedHashSet;"
        | "Ljava/util/TreeSet;"
        | "Ljava/util/concurrent/CopyOnWriteArraySet;" => Some("Ljava/util/AbstractSet;"),
        "Ljava/util/AbstractSet;" => Some("Ljava/util/AbstractCollection;"),
        "Ljava/util/AbstractCollection;" | "Ljava/util/AbstractMap;" => Some("Ljava/lang/Object;"),
        "Ljava/lang/Integer;"
        | "Ljava/lang/Long;"
        | "Ljava/lang/Float;"
        | "Ljava/lang/Double;"
        | "Ljava/lang/Byte;"
        | "Ljava/lang/Short;"
        | "Ljava/math/BigDecimal;"
        | "Ljava/math/BigInteger;"
        | "Ljava/util/concurrent/atomic/AtomicInteger;"
        | "Ljava/util/concurrent/atomic/AtomicLong;" => Some("Ljava/lang/Number;"),
        "Ljava/lang/String;"
        | "Ljava/lang/StringBuilder;"
        | "Ljava/lang/StringBuffer;"
        | "Ljava/lang/Number;"
        | "Ljava/lang/Boolean;"
        | "Ljava/lang/Character;"
        | "Ljava/util/UUID;"
        | "Ljava/util/Locale;"
        | "Ljava/net/URL;"
        | "Ljava/net/URI;"
        | "Ljava/net/InetAddress;"
        | "Ljava/util/BitSet;"
        | "Ljava/util/Currency;"
        | "Ljava/util/concurrent/atomic/AtomicBoolean;"
        | "Ljava/util/concurrent/atomic/AtomicIntegerArray;"
        | "Ljava/util/concurrent/atomic/AtomicLongArray;"
        | "Ljava/util/concurrent/locks/ReentrantLock;" => Some("Ljava/lang/Object;"),
        _ => None,
    }
}

pub(super) fn bootstrap_direct_interface_descriptors(
    class_descriptor: &str,
) -> &'static [&'static str] {
    // cite: libcore Class.java#getInterfaces
    if class_descriptor.starts_with('[') {
        return &["Ljava/lang/Cloneable;", "Ljava/io/Serializable;"];
    }
    match class_descriptor {
        // cite: libcore Class.java#Class
        "Ljava/lang/Class;" => &[
            "Ljava/lang/reflect/GenericDeclaration;",
            "Ljava/lang/reflect/Type;",
            "Ljava/lang/reflect/AnnotatedElement;",
            "Ljava/lang/invoke/TypeDescriptor$OfField;",
            "Ljava/io/Serializable;",
        ],
        // cite: libcore GenericDeclaration.java#GenericDeclaration
        "Ljava/lang/reflect/GenericDeclaration;" => &["Ljava/lang/reflect/AnnotatedElement;"],
        // cite: libcore TypeDescriptor.java#OfField
        "Ljava/lang/invoke/TypeDescriptor$OfField;" => &["Ljava/lang/invoke/TypeDescriptor;"],
        "Ljava/lang/String;" | "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;" => {
            &["Ljava/lang/CharSequence;", "Ljava/io/Serializable;"]
        }
        // cite: libcore Enum.java#Enum
        "Ljava/lang/Enum;" => &["Ljava/lang/Comparable;", "Ljava/io/Serializable;"],
        "Ljava/lang/Number;"
        | "Ljava/lang/Boolean;"
        | "Ljava/lang/Character;"
        | "Ljava/util/UUID;"
        | "Ljava/util/Locale;"
        | "Ljava/net/URL;"
        | "Ljava/net/URI;"
        | "Ljava/net/InetAddress;"
        | "Ljava/util/BitSet;"
        | "Ljava/util/Currency;"
        | "Ljava/util/concurrent/atomic/AtomicBoolean;"
        | "Ljava/util/concurrent/atomic/AtomicIntegerArray;"
        | "Ljava/util/concurrent/atomic/AtomicLongArray;" => &["Ljava/io/Serializable;"],
        "Ljava/util/Collection;" => &["Ljava/lang/Iterable;"],
        "Ljava/util/List;" | "Ljava/util/Queue;" | "Ljava/util/Set;" => &["Ljava/util/Collection;"],
        "Ljava/util/ArrayList;" | "Ljava/util/Arrays$ArrayList;" => &[
            "Ljava/util/List;",
            "Ljava/util/RandomAccess;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ],
        "Ljava/util/AbstractList;" => &["Ljava/util/List;"],
        "Ljava/util/PriorityQueue;" => &["Ljava/io/Serializable;"],
        "Ljava/util/AbstractQueue;" => &["Ljava/util/Queue;"],
        "Ljava/util/AbstractCollection;" => &["Ljava/util/Collection;"],
        "Ljava/util/HashSet;"
        | "Ljava/util/LinkedHashSet;"
        | "Ljava/util/TreeSet;"
        | "Ljava/util/concurrent/CopyOnWriteArraySet;"
        | "Ljava/util/AbstractSet;" => &["Ljava/util/Set;"],
        "Ljava/util/HashMap;"
        | "Ljava/util/IdentityHashMap;"
        | "Ljava/util/LinkedHashMap;"
        | "Ljava/util/TreeMap;"
        | "Ljava/util/WeakHashMap;"
        | "Ljava/util/concurrent/ConcurrentHashMap;"
        | "Ljava/util/AbstractMap;" => &["Ljava/util/Map;"],
        "Ljava/util/concurrent/locks/ReentrantLock;" => &[
            "Ljava/io/Serializable;",
            "Ljava/util/concurrent/locks/Lock;",
        ],
        // cite: frameworks-base WindowManagerImpl.java#WindowManagerImpl
        "Landroid/view/WindowManagerImpl;" => &["Landroid/view/WindowManager;"],
        "Landroid/os/Bundle;" => &["Ljava/lang/Cloneable;", "Landroid/os/Parcelable;"],
        // cite: frameworks-base AbstractCursor.java#AbstractCursor
        "Landroid/database/AbstractCursor;" => &["Landroid/database/CrossProcessCursor;"],
        // cite: frameworks-base CrossProcessCursor.java#CrossProcessCursor
        "Landroid/database/CrossProcessCursor;" => &["Landroid/database/Cursor;"],
        // cite: frameworks-base OnBackAnimationCallback.java#OnBackAnimationCallback
        "Landroid/window/OnBackAnimationCallback;" => &["Landroid/window/OnBackInvokedCallback;"],
        _ => &[],
    }
}

pub(super) fn is_bootstrap_interface_descriptor(class_descriptor: &str) -> bool {
    matches!(
        class_descriptor,
        "Ljava/lang/CharSequence;"
            | "Ljava/lang/Cloneable;"
            | "Ljava/io/Serializable;"
            | "Ljava/lang/Iterable;"
            | "Ljava/lang/Runnable;"
            | "Ljava/lang/reflect/Type;"
            | "Ljava/lang/reflect/GenericDeclaration;"
            | "Ljava/lang/reflect/AnnotatedElement;"
            // cite: libcore TypeDescriptor.java#TypeDescriptor
            | "Ljava/lang/invoke/TypeDescriptor;"
            | "Ljava/lang/invoke/TypeDescriptor$OfField;"
            | "Ljava/util/Collection;"
            | "Ljava/util/Iterator;"
            | "Ljava/util/List;"
            | "Ljava/util/Map;"
            | "Ljava/util/Map$Entry;"
            | "Ljava/util/Queue;"
            | "Ljava/util/RandomAccess;"
            | "Ljava/util/Set;"
            | "Ljava/util/concurrent/Executor;"
            | "Ljava/util/concurrent/ExecutorService;"
            | "Ljava/util/concurrent/Future;"
            // cite: frameworks-base OnBackAnimationCallback.java#OnBackAnimationCallback
            | "Landroid/window/OnBackAnimationCallback;"
            // cite: frameworks-base OnBackInvokedCallback.java#OnBackInvokedCallback
            | "Landroid/window/OnBackInvokedCallback;"
            // cite: frameworks-base WindowManager.java#WindowManager
            | "Landroid/view/WindowManager;"
            | "Ljava/util/concurrent/locks/Lock;"
            // cite: frameworks-base Cursor.java#Cursor
            | "Landroid/database/Cursor;"
            // cite: frameworks-base CrossProcessCursor.java#CrossProcessCursor
            | "Landroid/database/CrossProcessCursor;"
            | "Landroid/os/Parcelable;"
    )
}

pub(super) fn is_primitive_descriptor(class_descriptor: &str) -> bool {
    matches!(
        class_descriptor,
        "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D" | "V"
    )
}
