package com.example.app;

import com.example.util.Text;
import static com.example.util.Text.shout;
import com.example.unknown.*;
import java.util.List;

public class App {
    private final List<String> items = List.of(shout("hi"), new Text().render());
}
